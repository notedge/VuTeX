//! Layout tree (boxes, glue, kerns, glyphs) and the layout engine that lowers a
//! `SemanticNode` tree into it.

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use vutex_text::math::{FontMetricsSystem, GlyphId, KernCorner, MathFontFacts, StretchAxis, StretchPlan};
use vutex_types::{
    Diagnostic, Severity,
    math::{
        Alignment, AlignmentAnchors, BBox, DelimiterComponent, DelimiterVariant, Fixed, FontMetricsSnapshot, FontStyle,
        FormulaLayoutResult, FormulaMetrics, GlyphKey, GlyphMetrics, LimitsPolicy, LineStyle, MathConstant, MathEnvironment,
        MathItem, MathList, MathResult, MathStyle, MathStyleState, Noad, NoadKind, OperatorClass, PaintNode, SemanticNode,
        SpacingRule, SymbolRole, normalize_math_list, project_semantic_with_state,
    },
};

/// A node in the math layout tree.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum LayoutNode {
    /// A horizontal box.
    HBox(Box<HBox>),
    /// A vertical box.
    VBox(Box<VBox>),
    /// A single glyph.
    Glyph(Glyph),
    /// Flexible space.
    Glue(Glue),
    /// Fixed horizontal or vertical space.
    Kern(Fixed),
    /// A filled rectangle (fraction bars, radical rules).
    Rule {
        /// Rule width.
        width: Fixed,
        /// Rule height above the baseline.
        height: Fixed,
        /// Rule depth below the baseline.
        depth: Fixed,
    },
    /// A pre-scaled vector path.
    Path(Path),
    /// A line-break candidate with its penalty.
    Penalty {
        /// Width contributed by the penalty.
        width: Fixed,
        /// Penalty value; negative values encourage a break.
        penalty: f64,
        /// Whether the break is flagged (hyphenation-like).
        flagged: bool,
    },
}

impl LayoutNode {
    /// Natural advance width of the node.
    pub fn width(&self) -> Fixed {
        match self {
            LayoutNode::HBox(h) => h.width,
            LayoutNode::VBox(v) => v.width,
            LayoutNode::Glyph(g) => g.metrics.width + self.italic_correction(),
            LayoutNode::Glue(g) => g.width,
            LayoutNode::Kern(k) => *k,
            LayoutNode::Rule { width, .. } => *width,
            LayoutNode::Path(p) => p.width,
            LayoutNode::Penalty { width, .. } => *width,
        }
    }

    /// Italic correction that [`LayoutNode::width`] folds into the advance.
    ///
    /// Only italic and math-style glyphs contribute one; every other node
    /// advances by its bare width.
    pub fn italic_correction(&self) -> Fixed {
        match self {
            LayoutNode::Glyph(g) if matches!(g.font_style, FontStyle::Italic | FontStyle::Math) => g.metrics.italic_correction,
            _ => Fixed::ZERO,
        }
    }

    /// Extent above the baseline.
    pub fn height(&self) -> Fixed {
        match self {
            LayoutNode::HBox(h) => h.height - h.shift,
            LayoutNode::VBox(v) => v.height,
            LayoutNode::Glyph(g) => g.metrics.height,
            LayoutNode::Glue(g) => g.width, // Treat Glue width as height in VBox
            LayoutNode::Kern(k) => *k,      // Treat Kern width as height in VBox
            LayoutNode::Penalty { .. } => Fixed::ZERO,
            LayoutNode::Rule { height, .. } => *height,
            LayoutNode::Path(p) => p.height,
        }
    }

    /// Extent below the baseline.
    pub fn depth(&self) -> Fixed {
        match self {
            LayoutNode::HBox(h) => h.depth + h.shift,
            LayoutNode::VBox(v) => v.depth,
            LayoutNode::Glyph(g) => g.metrics.depth,
            LayoutNode::Glue(_) | LayoutNode::Kern(_) | LayoutNode::Penalty { .. } => Fixed::ZERO,
            LayoutNode::Rule { depth, .. } => *depth,
            LayoutNode::Path(p) => p.depth,
        }
    }

    /// Project the node onto the Knuth-Plass item model for line breaking.
    pub fn to_kp_item(&self) -> crate::math::knuth_plass::Item {
        match self {
            LayoutNode::Glue(g) => {
                crate::math::knuth_plass::Item::Glue { width: g.width, stretch: g.stretch, shrink: g.shrink }
            }
            LayoutNode::Penalty { width, penalty, flagged } => {
                crate::math::knuth_plass::Item::Penalty { width: *width, penalty: *penalty, flagged: *flagged }
            }
            LayoutNode::Kern(k) => {
                crate::math::knuth_plass::Item::Glue { width: *k, stretch: Fixed::ZERO, shrink: Fixed::ZERO }
            }
            _ => crate::math::knuth_plass::Item::Box { width: self.width(), debug_info: None },
        }
    }
}

/// A horizontal list of children with its own extents and baseline shift.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct HBox {
    /// Box width.
    pub width: Fixed,
    /// Extent above the baseline.
    pub height: Fixed,
    /// Extent below the baseline.
    pub depth: Fixed,
    /// Child nodes.
    pub children: Vec<LayoutNode>,
    /// Vertical baseline shift applied when rendering.
    pub shift: Fixed,
    /// Applied glue ratio (`> 0` stretched, `< 0` shrunk).
    pub glue_set: f64,
}

/// A vertical list of children with its own extents and baseline shift.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VBox {
    /// Box width.
    pub width: Fixed,
    /// Extent above the baseline.
    pub height: Fixed,
    /// Extent below the baseline.
    pub depth: Fixed,
    /// Child nodes.
    pub children: Vec<LayoutNode>,
    /// Horizontal baseline shift applied when rendering.
    pub shift: Fixed,
    /// Applied glue ratio (`> 0` stretched, `< 0` shrunk).
    pub glue_set: f64,
}

/// A single glyph with resolved metrics.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Glyph {
    /// Unicode scalar value when the glyph came from text.
    pub char: Option<char>,
    /// Font family name used for text output.
    pub font_family: String,
    /// Font style of the source glyph key — used to decide whether to
    /// consume `metrics.italic_correction` in the advance width.
    pub font_style: FontStyle,
    /// Em size in pixels.
    pub size: Fixed,
    /// Scaled metrics.
    pub metrics: GlyphMetrics,
    /// Pre-scaled SVG path data when the font provides outlines.
    pub path_data: Option<String>,
    /// Font unique identifier (family hash) matched against
    /// [`PaintNode::GlyphRun::font_id`].  `None` when the glyph came from a
    /// text fallback or the font system carried no face id.
    pub font_id: Option<String>,
    /// Glyph id within the referenced font face.  `None` when the GlyphKey
    /// lacked an id (e.g. a char-only reference) or the font could not
    /// resolve one.
    pub glyph_id: Option<u16>,
}

/// Flexible space that can stretch or shrink during packing.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Glue {
    /// Natural width.
    pub width: Fixed,
    /// Maximum stretch.
    pub stretch: Fixed,
    /// Maximum shrink.
    pub shrink: Fixed,
}

/// TeXbook Appendix G math spacing skip between two adjacent atoms.
///
/// Each cell of the 8×8 table over the atom classes (Ord, Op, Bin, Rel, Open,
/// Close, Punct, Inner) says whether to insert no glue, thin (3mu), medium
/// (4mu), or thick (5mu).  The lookup always runs after demotion has downgraded
/// Bin / Rel / Op atoms flanked by certain neighbours, so the table itself is
/// simpler than it first appears.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum TexSkip {
    NoGlue,
    /// `thinmuskip` = 3mu = 3/18 em.  Ordinary atoms next to operators,
    /// delimiters, punctuation and inner atoms.
    Thin,
    /// `medmuskip` = 4mu = 4/18 em.  Around binary operators.
    Medium,
    /// `thickmuskip` = 5mu = 5/18 em.  Around relations.
    Thick,
}

impl TexSkip {
    fn to_glue(self, size: Fixed) -> Option<Glue> {
        let em = |f: f64| Fixed::from_f64(f) * size;
        match self {
            TexSkip::NoGlue => None,
            TexSkip::Thin => Some(Glue { width: em(THIN_MUSIP_EM), stretch: Fixed::ZERO, shrink: Fixed::ZERO }),
            TexSkip::Medium => Some(Glue { width: em(MEDIUM_MUSIP_EM), stretch: Fixed::ZERO, shrink: Fixed::ZERO }),
            TexSkip::Thick => Some(Glue { width: em(THICK_MUSIP_EM), stretch: Fixed::ZERO, shrink: Fixed::ZERO }),
        }
    }
}

/// The eight atom classes of the TeXbook Appendix G spacing table.
///
/// [`NoadKind`] also carries structural kinds (fences, radicals, `\vcenter`
/// boxes and table cells) that TeX classifies as Inner for glue purposes, so
/// this projection gives the table a closed 8×8 domain and lets the lookup be
/// spelled out cell by cell.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum AtomClass {
    Ord,
    Op,
    Bin,
    Rel,
    Open,
    Close,
    Punct,
    Inner,
}

impl AtomClass {
    fn of(kind: NoadKind) -> Self {
        match kind {
            NoadKind::Atom => AtomClass::Ord,
            NoadKind::Op => AtomClass::Op,
            NoadKind::Bin => AtomClass::Bin,
            NoadKind::Rel => AtomClass::Rel,
            NoadKind::Open => AtomClass::Open,
            NoadKind::Close => AtomClass::Close,
            NoadKind::Punct => AtomClass::Punct,
            NoadKind::Inner
            | NoadKind::Fence
            | NoadKind::Radical
            | NoadKind::Vcent
            | NoadKind::TableCell
            | NoadKind::Environment => AtomClass::Inner,
        }
    }
}

/// A vector path scaled to the current font size.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Path {
    /// SVG path data.
    pub d: String,
    /// Path advance width.
    pub width: Fixed,
    /// Extent above the baseline.
    pub height: Fixed,
    /// Extent below the baseline.
    pub depth: Fixed,
}

/// Rendering sink consumed by [`render_layout_node`].
pub trait LayoutBackend {
    /// Emit text at the given baseline position.
    fn render_text(&mut self, text: &str, x: f64, y: f64, font_size: f64, font_family: Option<&str>) -> MathResult<()>;

    /// Emit a filled rectangle.
    fn render_rect(&mut self, x: f64, y: f64, w: f64, h: f64) -> MathResult<()>;

    /// Emit a vector path scaled by `scale`.
    fn render_path(&mut self, d: &str, x: f64, y: f64, scale: f64) -> MathResult<()>;

    /// Open a group, optionally with an SVG transform string.
    fn start_group(&mut self, transform: Option<&str>) -> MathResult<()>;

    /// Close the innermost group.
    fn end_group(&mut self) -> MathResult<()>;

    /// When true, Glyph nodes should emit text (for semantic backends like
    /// MathML that need role-aware tags), not outline paths.
    #[allow(unused_variables)]
    fn prefers_text_over_path(&self) -> bool {
        false
    }
}

/// Whether painting this node puts any ink on the page.
///
/// Glue, kerns and penalties only advance, and a glyph the font could not
/// outline carries no path, so a box built from those alone is blank.
fn paints(node: &LayoutNode) -> bool {
    match node {
        LayoutNode::Glyph(glyph) => glyph.path_data.is_some() || glyph.char.is_some(),
        LayoutNode::Rule { .. } | LayoutNode::Path(_) => true,
        LayoutNode::HBox(hbox) => hbox.children.iter().any(paints),
        LayoutNode::VBox(vbox) => vbox.children.iter().any(paints),
        LayoutNode::Glue(_) | LayoutNode::Kern(_) | LayoutNode::Penalty { .. } => false,
    }
}

/// Recursively render a layout tree into a backend at the given baseline position.
pub fn render_layout_node(backend: &mut dyn LayoutBackend, node: &LayoutNode, x: f64, y: f64) -> MathResult<()> {
    match node {
        LayoutNode::HBox(hbox) => {
            // A box whose subtree paints nothing (an unused alignment cell, a
            // box of kerns and glue) must not open a group that the SVG backend
            // would emit as a blank `<g>`.
            if !hbox.children.iter().any(paints) {
                return Ok(());
            }
            backend.start_group(Some(&format!("translate({}, {})", x, y + hbox.shift.to_f64())))?;
            let mut current_x = 0.0;
            for child in &hbox.children {
                render_layout_node(backend, child, current_x, 0.0)?;
                current_x += child.width().to_f64();
            }
            backend.end_group()?;
        }
        LayoutNode::VBox(vbox) => {
            if !vbox.children.iter().any(paints) {
                return Ok(());
            }
            backend.start_group(Some(&format!("translate({}, {})", x + vbox.shift.to_f64(), y)))?;
            let mut current_y = -vbox.height.to_f64();
            for child in &vbox.children {
                render_layout_node(backend, child, 0.0, current_y + child.height().to_f64())?;
                current_y += child.height().to_f64() + child.depth().to_f64();
            }
            backend.end_group()?;
        }
        LayoutNode::Glyph(glyph) => {
            // Path-first for render targets (SVG), text-first for semantic
            // targets (MathML that needs role-aware tags like <mo>/<mi>).
            if !backend.prefers_text_over_path() {
                if let Some(ref path) = glyph.path_data {
                    backend.render_path(path, x, y, glyph.size.to_f64())?;
                }
                else if let Some(c) = glyph.char {
                    let text = c.to_string();
                    backend.render_text(&text, x, y, glyph.size.to_f64(), Some(&glyph.font_family))?;
                }
            }
            else if let Some(c) = glyph.char {
                let text = c.to_string();
                backend.render_text(&text, x, y, glyph.size.to_f64(), Some(&glyph.font_family))?;
            }
        }
        LayoutNode::Rule { width, height, depth } => {
            backend.render_rect(x, y - height.to_f64(), width.to_f64(), height.to_f64() + depth.to_f64())?;
        }
        LayoutNode::Path(path) => {
            backend.render_path(&path.d, x, y, 1.0)?;
        }
        LayoutNode::Kern(_) | LayoutNode::Glue(_) | LayoutNode::Penalty { .. } => {
            // Kerns, Glue and Penalty don't render anything
        }
    }
    Ok(())
}

/// The accent character `\bar` / `\overline` is built from.
///
/// Unlike `\hat`, `\tilde`, `\dot`, `\ddot` and `\vec`, an overbar is a rule
/// spanning the nucleus rather than a letterform, so it never paints a glyph.
const OVERBAR_ACCENT: char = '¯';

/// 1 mu = 1/18 em. TeX thick/medium/thin muskip in em units.
const THICK_MUSIP_EM: f64 = 5.0 / 18.0;
const MEDIUM_MUSIP_EM: f64 = 4.0 / 18.0;
const THIN_MUSIP_EM: f64 = 3.0 / 18.0;

/// The embedded math font's `MATH` facts, parsed once per process.
///
/// The facts answer the questions the metrics cache cannot: which prepared
/// design covers a target size, how an assembly repeats its connectors, and
/// what a `MathKernInfo` corner publishes at a correction height. Parsing the
/// font is expensive enough that it must not happen per formula.
fn font_facts() -> Option<&'static MathFontFacts> {
    static FACTS: OnceLock<Option<MathFontFacts>> = OnceLock::new();
    FACTS.get_or_init(MathFontFacts::embedded).as_ref()
}

/// The embedded font's glyph for a layout key.
///
/// `None` for a key that names another family or a character the font does not
/// publish, so a caller can keep its metrics-cache route instead of guessing.
fn font_facts_glyph(key: &GlyphKey) -> Option<GlyphId> {
    let family = key.font_family.as_deref().unwrap_or("default");
    if family != "default" {
        return None;
    }
    match (key.glyph_id, key.char) {
        (Some(glyph_id), _) => Some(GlyphId::new(glyph_id)),
        (None, Some(c)) => font_facts()?.glyph_for_char(c),
        (None, None) => None,
    }
}

/// Lowers [`SemanticNode`] trees into [`LayoutNode`] boxes.
#[derive(Debug)]
pub struct LayoutEngine {
    font_system: FontMetricsSystem,
    base_size: Fixed,
}

impl LayoutEngine {
    /// Create an engine with a base em size of `1.0`.
    pub fn new(font_system: FontMetricsSystem) -> Self {
        Self { font_system, base_size: Fixed::ONE }
    }

    /// Set the base em size in pixels.
    pub fn with_base_size(mut self, size: f64) -> Self {
        self.base_size = Fixed::from_f64(size);
        self
    }

    /// Best-effort role of a semantic node for spacing purposes.
    ///
    /// Nodes that aren't atoms (Fraction, Radical, Subsuperscript, Matrix, etc.)
    /// behave like TeX Inner atoms — spacing flows around them.
    fn role_of(node: &SemanticNode) -> SymbolRole {
        match node {
            SemanticNode::Symbol { role, .. } => *role,
            SemanticNode::Subscript { .. }
            | SemanticNode::Superscript { .. }
            | SemanticNode::SubSuperscript { .. }
            | SemanticNode::Fraction { .. }
            | SemanticNode::Radical { .. }
            | SemanticNode::Delimited { .. }
            | SemanticNode::Matrix { .. }
            | SemanticNode::Environment(_)
            | SemanticNode::Accent { .. }
            | SemanticNode::HorizontalBox { .. }
            | SemanticNode::VerticalBox { .. }
            | SemanticNode::Paragraph { .. } => SymbolRole::Inner,
            SemanticNode::Sequence(_)
            | SemanticNode::Text { .. }
            | SemanticNode::Space { .. }
            | SemanticNode::SetMathStyle(_)
            | SemanticNode::LimitsSpec(_) => SymbolRole::Ordinary,
        }
    }

    /// The first [`GlyphKey`] reachable from a semantic node.
    ///
    /// Walks the tree skipping `Sequence`, style commands, delimiters and
    /// script wrappers until it hits a `Symbol` or `Text` with at least one
    /// character.  Returns `None` for purely structural nodes (fractions,
    /// radicals, matrices, …) that don't directly own a glyph key.
    fn first_glyph_key(node: &SemanticNode) -> Option<GlyphKey> {
        match node {
            SemanticNode::Symbol { glyph_key, .. } => Some(glyph_key.clone()),
            SemanticNode::Sequence(items) => items.iter().find_map(Self::first_glyph_key),
            SemanticNode::Text { text, font_style } => text.chars().next().map(|c| GlyphKey::from_char(c, None, *font_style)),
            SemanticNode::Subscript { base, .. }
            | SemanticNode::Superscript { base, .. }
            | SemanticNode::SubSuperscript { base, .. } => Self::first_glyph_key(base),
            SemanticNode::Fraction { num, .. } => Self::first_glyph_key(num),
            SemanticNode::Radical { radicand, .. } => Self::first_glyph_key(radicand),
            SemanticNode::Delimited { content, .. } => Self::first_glyph_key(content),
            SemanticNode::Accent { base, .. } => Self::first_glyph_key(base),
            SemanticNode::HorizontalBox { content, .. } => content.iter().find_map(Self::first_glyph_key),
            SemanticNode::Paragraph { content, .. } => content.iter().find_map(Self::first_glyph_key),
            SemanticNode::VerticalBox { content, .. } => content.iter().find_map(Self::first_glyph_key),
            SemanticNode::Matrix { rows, .. } => rows.iter().flatten().find_map(Self::first_glyph_key),
            SemanticNode::Environment(environment) => {
                environment.rows.iter().flat_map(|row| row.cells.iter()).find_map(Self::first_glyph_key)
            }
            SemanticNode::Space { .. } | SemanticNode::SetMathStyle(_) | SemanticNode::LimitsSpec(_) => None,
        }
    }

    /// The `MathKernInfo` kern at one corner of `key`, in absolute units.
    ///
    /// `correction_height` is how far the other glyph's ink reaches away from the
    /// base baseline, in the same units. Returns `None` when the font publishes no
    /// kern at that corner, which is different from a published zero.
    fn math_kern(&self, key: &GlyphKey, corner: KernCorner, correction_height: Fixed, size: Fixed) -> Option<Fixed> {
        let facts = font_facts()?;
        let glyph = font_facts_glyph(key)?;
        let kerns = facts.kerns();
        kerns.corner(glyph, corner).is_some().then(|| kerns.math_kern(glyph, corner, correction_height / size) * size)
    }

    /// The `MathKernInfo` kern between two adjacent atoms, in absolute units.
    ///
    /// TeX kerns the facing corners: the left glyph's bottom-right at the
    /// following glyph's depth, and the following glyph's top-left at the left
    /// glyph's height. `None` when the font publishes neither corner.
    fn math_kern_between(&self, left: &GlyphKey, right: &GlyphKey, size: Fixed) -> Option<Fixed> {
        let left_metrics = self.font_system.get_metrics(left).ok()?;
        let right_metrics = self.font_system.get_metrics(right).ok()?;
        let left_corner = self.math_kern(left, KernCorner::BottomRight, right_metrics.depth, size);
        let right_corner = self.math_kern(right, KernCorner::TopLeft, left_metrics.height, size);
        match (left_corner, right_corner) {
            (None, None) => None,
            (left_kern, right_kern) => Some(left_kern.unwrap_or(Fixed::ZERO) + right_kern.unwrap_or(Fixed::ZERO)),
        }
    }

    /// The font's `MathGlyphInfo` italic correction for `key`, in em.
    ///
    /// The metrics cache answers zero for a glyph it never extracted, so the
    /// `MATH` table is the only place an italic correction can be taken from
    /// without inventing one.
    fn facts_italic_correction(&self, key: &GlyphKey) -> Option<Fixed> {
        let facts = font_facts()?;
        facts.italic_correction(font_facts_glyph(key)?)
    }

    /// First atom of `node` that takes part in math spacing, skipping the
    /// imperative nodes that produce no boxes.
    fn leading_role(node: &SemanticNode) -> Option<SymbolRole> {
        match node {
            SemanticNode::Sequence(items) => items.iter().find_map(Self::leading_role),
            SemanticNode::SetMathStyle(_) | SemanticNode::LimitsSpec(_) => None,
            other => Some(Self::role_of(other)),
        }
    }

    /// TeXbook Appendix G spacing table keyed by [`NoadKind`].
    ///
    /// This is the unique source of truth for inter-atom math glue.  Rows are
    /// the left atom's class and columns the right atom's class, in the
    /// canonical order Ord, Op, Bin, Rel, Open, Close, Punct, Inner.  Because
    /// [`normalize_math_list`] runs the demotion pass before the spacing phase,
    /// this function consumes already-resolved kinds and follows Knuth's table
    /// cell by cell — it never re-derives a class from a source role.  Blank
    /// cells (demoted away in practice) are spelled out as `NoGlue`.
    fn tex_skip_between_noad(left: NoadKind, right: NoadKind) -> TexSkip {
        use AtomClass::*;
        use TexSkip::*;

        let (left, right) = (AtomClass::of(left), AtomClass::of(right));
        match (left, right) {
            // Ord row: 0 1 2 3 0 0 0 1
            (Ord, Ord) => NoGlue,
            (Ord, Op) => Thin,
            (Ord, Bin) => Medium,
            (Ord, Rel) => Thick,
            (Ord, Open) => NoGlue,
            (Ord, Close) => NoGlue,
            (Ord, Punct) => NoGlue,
            (Ord, Inner) => Thin,
            // Op row: 1 1 - 3 0 0 0 1
            (Op, Ord) => Thin,
            (Op, Op) => Thin,
            (Op, Bin) => NoGlue,
            (Op, Rel) => Thick,
            (Op, Open) => NoGlue,
            (Op, Close) => NoGlue,
            (Op, Punct) => NoGlue,
            (Op, Inner) => Thin,
            // Bin row: 2 2 - - 2 - - 2
            (Bin, Ord) => Medium,
            (Bin, Op) => Medium,
            (Bin, Bin) => NoGlue,
            (Bin, Rel) => NoGlue,
            (Bin, Open) => Medium,
            (Bin, Close) => NoGlue,
            (Bin, Punct) => NoGlue,
            (Bin, Inner) => Medium,
            // Rel row: 3 3 - 0 3 0 0 3
            (Rel, Ord) => Thick,
            (Rel, Op) => Thick,
            (Rel, Bin) => NoGlue,
            (Rel, Rel) => NoGlue,
            (Rel, Open) => Thick,
            (Rel, Close) => NoGlue,
            (Rel, Punct) => NoGlue,
            (Rel, Inner) => Thick,
            // Open row: 0 0 - 0 0 0 0 0
            (Open, Ord) => NoGlue,
            (Open, Op) => NoGlue,
            (Open, Bin) => NoGlue,
            (Open, Rel) => NoGlue,
            (Open, Open) => NoGlue,
            (Open, Close) => NoGlue,
            (Open, Punct) => NoGlue,
            (Open, Inner) => NoGlue,
            // Close row: 0 1 2 3 0 0 0 1
            (Close, Ord) => NoGlue,
            (Close, Op) => Thin,
            (Close, Bin) => Medium,
            (Close, Rel) => Thick,
            (Close, Open) => NoGlue,
            (Close, Close) => NoGlue,
            (Close, Punct) => NoGlue,
            (Close, Inner) => Thin,
            // Punct row: 1 1 - 1 1 1 1 1
            (Punct, Ord) => Thin,
            (Punct, Op) => Thin,
            (Punct, Bin) => NoGlue,
            (Punct, Rel) => Thin,
            (Punct, Open) => Thin,
            (Punct, Close) => Thin,
            (Punct, Punct) => Thin,
            (Punct, Inner) => Thin,
            // Inner row: 1 1 2 3 1 0 1 1
            (Inner, Ord) => Thin,
            (Inner, Op) => Thin,
            (Inner, Bin) => Medium,
            (Inner, Rel) => Thick,
            (Inner, Open) => Thin,
            (Inner, Close) => NoGlue,
            (Inner, Punct) => Thin,
            (Inner, Inner) => Thin,
        }
    }

    /// Lay out a semantic node in the given math style.
    pub fn layout_node<'a>(&'a self, node: &'a SemanticNode, style: MathStyle) -> MathResult<LayoutNode> {
        self.layout_node_state(node, MathStyleState::from_style(style))
    }

    /// Lay out a semantic node in a recursive style state.
    ///
    /// `cramped` travels with the style because it is not derivable from
    /// [`MathStyle`] alone: fraction denominators, radical radicands and
    /// subscripts raise it, while superscripts, numerators and plain sequence
    /// bodies inherit it.  A superscript's shift picks
    /// `SuperscriptShiftUpCramped` over `SuperscriptShiftUp` accordingly.
    fn layout_node_state<'a>(&'a self, node: &'a SemanticNode, state: MathStyleState) -> MathResult<LayoutNode> {
        {
            match node {
                SemanticNode::Symbol { glyph_key, role } => self.layout_math_symbol(glyph_key, *role, state.style),
                SemanticNode::Sequence(nodes) => self.layout_sequence(nodes, state),
                SemanticNode::HorizontalBox { content, spacing } => {
                    let mut children = Vec::new();
                    let n = content.len();
                    for (i, node) in content.iter().enumerate() {
                        children.push(self.layout_node_state(node, state)?);
                        if i + 1 < n {
                            if let Some(glue) = self.get_spacing_glue(*spacing) {
                                children.push(LayoutNode::Glue(glue));
                            }
                        }
                    }
                    Ok(self.pack_hbox(children, None))
                }
                SemanticNode::Paragraph { content, width } => {
                    let mut children = Vec::new();
                    let n = content.len();
                    for (i, node) in content.iter().enumerate() {
                        children.push(self.layout_node_state(node, state)?);
                        if i + 1 < n {
                            // Add a default space (glue) between nodes in a paragraph
                            children.push(LayoutNode::Glue(Glue {
                                width: Fixed::from_f64(3.0),
                                stretch: Fixed::from_f64(3.0),
                                shrink: Fixed::from_f64(1.0),
                            }));
                        }
                    }
                    Ok(self.break_lines(children, vec![*width], 1.0))
                }
                SemanticNode::Fraction { num, den, line } => self.layout_fraction(num, den, *line, state),
                SemanticNode::Radical { degree, radicand } => self.layout_radical(degree.as_deref(), radicand, state),
                SemanticNode::VerticalBox { content, alignment } => {
                    let mut children = Vec::new();
                    for node in content {
                        children.push(self.layout_node_state(node, state)?);
                    }
                    Ok(self.pack_vbox(children, *alignment))
                }
                SemanticNode::Text { text, font_style } => {
                    let mut children = Vec::new();
                    let size = self.base_size * self.get_style_scale(state.style);
                    for c in text.chars() {
                        let key = GlyphKey::from_char(c, None, *font_style);
                        // Whitespace is a horizontal advance, not a glyph: TeX
                        // sets the spaces `\,`, `\:` and `\ ` as kerns, and a
                        // blank glyph would be reported as a missing outline
                        // without ever painting ink.
                        if c.is_whitespace() {
                            children.push(LayoutNode::Kern(self.font_system.get_metrics(&key)?.width * size));
                        }
                        else {
                            children.push(self.layout_symbol(&key, state.style)?);
                        }
                    }
                    Ok(self.pack_hbox(children, None))
                }
                // A script node reached directly (outside the noad pipeline)
                // carries no `\limits` information of its own, so placement
                // follows the operator class default and the named-operator
                // name table.
                SemanticNode::Subscript { base, sub } => {
                    self.layout_subsup(base, Some(sub), None, state, OperatorClass::of_node(base), LimitsPolicy::of_node(base))
                }
                SemanticNode::Superscript { base, sup } => {
                    self.layout_subsup(base, None, Some(sup), state, OperatorClass::of_node(base), LimitsPolicy::of_node(base))
                }
                SemanticNode::SubSuperscript { base, sub, sup } => self.layout_subsup(
                    base,
                    Some(sub),
                    Some(sup),
                    state,
                    OperatorClass::of_node(base),
                    LimitsPolicy::of_node(base),
                ),
                // A math space is a kern of the current em: `width` is already
                // in em, so the style scale is the only factor left to apply.
                SemanticNode::Space { width } => {
                    let size = self.base_size * self.get_style_scale(state.style);
                    Ok(LayoutNode::Kern(*width * size))
                }
                SemanticNode::Matrix { rows, row_spacing, col_spacing, display_style, column_alignment } => {
                    // `align` / `align*` pin their cells to display style so an
                    // enclosing `\scriptstyle` cannot demote the grid.  The
                    // cramped flag still follows the surrounding formula.
                    let state = if *display_style { MathStyleState { style: MathStyle::Display, ..state } } else { state };
                    let rows: Vec<&[SemanticNode]> = rows.iter().map(|row| row.as_slice()).collect();
                    self.layout_matrix(&rows, *row_spacing, *col_spacing, column_alignment, state)
                }
                SemanticNode::Environment(environment) => self.layout_environment(environment, state),
                SemanticNode::Delimited { left, right, content } => {
                    self.layout_delimited(left.as_ref(), right.as_ref(), content, state)
                }
                SemanticNode::Accent { accent, base } => self.layout_accent(accent, base, state),
                // Imperative nodes: no boxes; side-effects are handled by
                // `layout_sequence` and `layout_subsup` when they iterate.
                SemanticNode::SetMathStyle(_) | SemanticNode::LimitsSpec(_) => Ok(LayoutNode::Kern(Fixed::ZERO)),
            }
        }
    }

    /// Lay out a Sequence — currently a thin delegation to the normalized
    /// [`layout_math_list`] pipeline (project → demote → layout-by-noads).
    /// Kept as a named method so callers don't need to know the pipeline
    /// name; internals can be swapped later without changing call sites.
    fn layout_sequence(&self, nodes: &[SemanticNode], state: MathStyleState) -> MathResult<LayoutNode> {
        self.layout_math_list_state(nodes, state)
    }

    /// New normalized pipeline: project → normalize → layout by noads.
    ///
    /// [`normalize_math_list`] runs the TeX Appendix G demote pass before
    /// this entry point is called, so the spacing phase in [`layout_noads`]
    /// only needs to key off the already-resolved [`NoadKind`].
    pub fn layout_math_list(&self, nodes: &[SemanticNode], style: MathStyle) -> MathResult<LayoutNode> {
        self.layout_math_list_state(nodes, MathStyleState::from_style(style))
    }

    /// [`layout_math_list`] for callers that already track `cramped`.
    ///
    /// The state has to reach [`project_semantic_with_state`], otherwise the
    /// projected noads lose the cramped flag their scripts and nested
    /// structures inherit from the enclosing fraction or radical.
    fn layout_math_list_state(&self, nodes: &[SemanticNode], state: MathStyleState) -> MathResult<LayoutNode> {
        let projected = project_semantic_with_state(nodes, state);
        let normalized = normalize_math_list(projected);
        self.layout_noads(&normalized)
    }

    /// 新入口：直接产出不可变 [`FormulaLayoutResult`]。
    ///
    /// 保持现有 `layout_noads → LayoutNode` 路径完全不变；本方法只是在
    /// 计算完 LayoutNode 树之后再跑一遍 lower + extract，把它降成
    /// `PaintNode` 树 + `FormulaMetrics`。
    pub fn layout_formula_result(&self, nodes: &[SemanticNode], style: MathStyle) -> MathResult<FormulaLayoutResult> {
        let layout_node = self.layout_math_list(nodes, style)?;
        let paint = lower_layout_to_paint(&layout_node, self.base_size);
        let metrics = extract_metrics(&layout_node, &self.font_system);
        let mut glyph_paths = Vec::new();
        collect_glyph_paths(&layout_node, &mut glyph_paths);
        let fonts = FontMetricsSnapshot { faces: Vec::new(), glyph_paths };
        Ok(FormulaLayoutResult { metrics, paint, diagnostics: self.script_scale_diagnostics(), fonts })
    }

    /// Lay out a flat, normalized [`MathList`].
    ///
    /// The list interleaves atoms with explicit glue.  Appendix G spacing is
    /// keyed on the atoms on either side of a gap — glue in between does not
    /// break that adjacency — and an explicit space only adds its own width.
    /// Each nucleus is lowered via [`layout_node`](Self::layout_node) or
    /// [`layout_subsup`](Self::layout_subsup) depending on whether scripts are
    /// attached.
    fn layout_noads(&self, list: &MathList) -> MathResult<LayoutNode> {
        Ok(self.pack_hbox(self.layout_noad_run(&list.0)?, None))
    }

    /// Lower one run of math-list items into the horizontal children between
    /// two boxes, with the inter-atom glue already inserted.
    ///
    /// A `\left…\right` pair reaches this list as `Open` · body · `Close` atoms
    /// so the Appendix G columns keep seeing a fence as an Open / Close atom.
    /// The delimiters themselves, though, have to be sized against the whole
    /// body between them, so the pair is laid out through [`Self::layout_fence`]
    /// and the body is lowered as a run of its own.
    fn layout_noad_run(&self, items: &[MathItem]) -> MathResult<Vec<LayoutNode>> {
        let mut children = Vec::new();
        let n = items.len();
        let mut i = 0;
        // Previous atom, not previous item: explicit glue sits between atoms
        // without becoming one of them, exactly as `\mskip` glue does in TeX.
        let mut previous: Option<&Noad> = None;

        while i < n {
            let noad = match &items[i] {
                MathItem::Glue(glue) => {
                    // The glue carries only its own advance.  It has no
                    // stretch/shrink in the IR, so it lowers as a fixed kern in
                    // the em of the style that authored it.
                    let size = self.base_size * self.get_style_scale(glue.style);
                    children.push(LayoutNode::Kern(glue.width * size));
                    i += 1;
                    continue;
                }
                MathItem::Noad(noad) => noad,
            };
            let state = MathStyleState { style: noad.style, cramped: noad.cramped };
            let style = state.style;

            // spacing glue between the previous atom and this one.
            // Effective kind already collapses structural kinds (Fence → Inner
            // etc.) and is the spacing class the table expects.
            if let Some(left) = previous {
                let left_kind = left.effective_kind();
                let right_kind = noad.effective_kind();
                let style_scale = self.get_style_scale(style);
                let size = self.base_size * style_scale;
                let skip = Self::tex_skip_between_noad(left_kind, right_kind);

                // Appendix G glue.
                if let Some(glue) = skip.to_glue(size) {
                    children.push(LayoutNode::Glue(glue));
                }

                // MathKern supplement on top of Appendix G glue. TeX reads the
                // corner that faces the neighbour: the left glyph's bottom-right
                // at the following glyph's depth, and the following glyph's
                // top-left at the left glyph's height, each through the font's
                // correction-height step function. A font that publishes neither
                // corner adds no kern at all.
                let left_nucleus = left.nucleus.as_deref();
                let right_nucleus = noad.nucleus.as_deref();
                if let (Some(left_node), Some(right_node)) = (left_nucleus, right_nucleus) {
                    if let (Some(left_key), Some(right_key)) =
                        (Self::first_glyph_key(left_node), Self::first_glyph_key(right_node))
                    {
                        if let Some(kern) = self.math_kern_between(&left_key, &right_key, size) {
                            if kern != Fixed::ZERO {
                                children.push(LayoutNode::Kern(kern));
                            }
                        }
                    }
                }
            }

            // A fence pair is lowered as one box: both delimiters are built
            // from the body's extents, so neither depends on what precedes or
            // follows the pair in this formula.
            if noad.fence && noad.kind == NoadKind::Open {
                if let Some(close) = Self::fence_close(items, i) {
                    let left = Self::delimiter_key(items[i].as_noad().and_then(|noad| noad.nucleus.as_deref()));
                    let right = Self::delimiter_key(items[close].as_noad().and_then(|noad| noad.nucleus.as_deref()));
                    let body = self.pack_hbox(self.layout_noad_run(&items[i + 1..close])?, None);
                    children.push(self.layout_fence(left.as_ref(), right.as_ref(), body, state)?);
                    // The pair ends on its `Close` atom, which is the left
                    // neighbour of whatever follows the fence.
                    previous = items[close].as_noad();
                    i = close + 1;
                    continue;
                }
            }

            // lay out nucleus + optional scripts.
            let node = match (&noad.nucleus, &noad.scripts) {
                (Some(nucleus), Some(scripts)) => {
                    let sub_ref = scripts.sub.as_deref();
                    let sup_ref = scripts.sup.as_deref();
                    // Both the operator class and the `\limits` / `\nolimits`
                    // policy come straight from this noad, so script placement
                    // never depends on the order noads are lowered in.  An
                    // authored policy wins, otherwise the nucleus supplies its
                    // own default (large-operator class or named-operator name).
                    self.layout_subsup(
                        nucleus,
                        sub_ref,
                        sup_ref,
                        state,
                        OperatorClass::of_node(nucleus),
                        noad.limits.unwrap_or_else(|| LimitsPolicy::of_node(nucleus)),
                    )?
                }
                (Some(nucleus), None) => self.layout_node_state(nucleus, state)?,
                (None, Some(scripts)) => {
                    let sub_ref = scripts.sub.as_deref();
                    let sup_ref = scripts.sup.as_deref();
                    // No nucleus → call layout_subsup with a zero-width kern as
                    // the base so the scripts still get positioned correctly.
                    self.layout_subsup(
                        &SemanticNode::SetMathStyle(style),
                        sub_ref,
                        sup_ref,
                        state,
                        OperatorClass::Ord,
                        LimitsPolicy::Auto,
                    )?
                }
                (None, None) => LayoutNode::Kern(Fixed::ZERO),
            };
            children.push(node);
            previous = Some(noad);
            i += 1;
        }

        Ok(children)
    }

    /// Index of the `Close` atom that shuts the fence opened at `open`.
    ///
    /// Nested pairs are paired innermost-first, exactly like the source `\left`
    /// … `\right` nesting they came from.  Glue items are skipped: only atoms
    /// open and close a fence.
    fn fence_close(items: &[MathItem], open: usize) -> Option<usize> {
        let mut depth = 0usize;
        for (offset, item) in items[open + 1..].iter().enumerate() {
            let Some(noad) = item.as_noad()
            else {
                continue;
            };
            if !noad.fence {
                continue;
            }
            match noad.kind {
                NoadKind::Open => depth += 1,
                NoadKind::Close => {
                    if depth == 0 {
                        return Some(open + 1 + offset);
                    }
                    depth -= 1;
                }
                _ => {}
            }
        }
        None
    }

    /// Delimiter glyph a fence noad carries as its synthetic symbol nucleus.
    fn delimiter_key(nucleus: Option<&SemanticNode>) -> Option<GlyphKey> {
        match nucleus? {
            SemanticNode::Symbol { glyph_key, .. } => Some(glyph_key.clone()),
            _ => None,
        }
    }

    fn layout_symbol(&self, key: &GlyphKey, style: MathStyle) -> MathResult<LayoutNode> {
        let style_scale = self.get_style_scale(style);
        let size = self.base_size * style_scale;
        self.layout_scaled_symbol(key, size)
    }

    /// Lay out a math symbol, preferring the font's display-size design for a
    /// large operator.
    ///
    /// TeX typesets `\sum` and friends with a taller glyph in display style and
    /// keeps the text-size design otherwise. MATH publishes those designs as the
    /// size variants of the operator's glyph construction, so they come from the
    /// font instead of being scaled up here.
    fn layout_math_symbol(&self, key: &GlyphKey, role: SymbolRole, style: MathStyle) -> MathResult<LayoutNode> {
        if matches!(role, SymbolRole::LargeOperator) && matches!(style, MathStyle::Display) {
            if let Some(variant) = self.display_operator_variant(key)? {
                let size = self.base_size * self.get_style_scale(style);
                return self.layout_scaled_symbol(&variant, size);
            }
        }
        self.layout_symbol(key, style)
    }

    /// Size variant of a large operator that reaches `DisplayOperatorMinHeight`,
    /// else the largest design the font prepared.
    ///
    /// The variants live in the same MATH construction table delimiters stretch
    /// from. TeX's constant is a minimum, not a stretch target: when the tallest
    /// operator design still falls short, that design is used as-is and nothing
    /// is synthesised. A font without a construction keeps the base glyph.
    fn display_operator_variant(&self, key: &GlyphKey) -> MathResult<Option<GlyphKey>> {
        let construction = self.font_system.get_delimiter_construction(key)?;
        let min_height = self.font_system.get_math_constant("default", MathConstant::DisplayOperatorMinHeight)?.to_f64();
        let mut tallest: Option<(Fixed, GlyphKey)> = None;
        for variant in construction.variants {
            let metrics = self.font_system.get_metrics(&variant.glyph)?;
            let total = metrics.height + metrics.depth;
            if total.to_f64() >= min_height {
                return Ok(Some(variant.glyph));
            }
            if tallest.as_ref().is_none_or(|(best, _)| total > *best) {
                tallest = Some((total, variant.glyph));
            }
        }
        Ok(tallest.map(|(_, glyph)| glyph))
    }

    fn layout_scaled_symbol(&self, key: &GlyphKey, size: Fixed) -> MathResult<LayoutNode> {
        let metrics = self.font_system.get_metrics(key)?;
        let path_data = self.font_system.get_glyph_path(key).ok();
        let font_family = key.font_family.clone().unwrap_or_else(|| "default".to_string());
        // `font_id` echoes the family name for now — the font snapshot layer
        // can refine this to a hash later.  `glyph_id` comes straight from
        // the GlyphKey; text-only keys carry None here.
        let font_id = Some(font_family.clone());
        let glyph_id = key.glyph_id;

        Ok(LayoutNode::Glyph(Glyph {
            char: key.char,
            font_family,
            font_style: key.style,
            size,
            metrics: GlyphMetrics {
                width: metrics.width * size.to_f64(),
                height: metrics.height * size.to_f64(),
                depth: metrics.depth * size.to_f64(),
                italic_correction: metrics.italic_correction * size.to_f64(),
                top_accent_attachment: metrics.top_accent_attachment * size.to_f64(),
            },
            path_data,
            font_id,
            glyph_id,
        }))
    }

    /// Em scale of a math style relative to the base size.
    ///
    /// `Display` and `Text` both set the base em, so they scale by exactly
    /// `1.0`. The two script levels are ratios the font publishes as
    /// `ScriptPercentScaleDown` / `ScriptScriptPercentScaleDown` in its `MATH`
    /// table, stored as fractions of the base em. Each ratio multiplies the base
    /// size directly, the same nesting TeX applies when it demotes a formula one
    /// level at a time.
    ///
    /// A font that publishes no ratio — a synthetic metrics table, or a font
    /// whose `MATH` table failed to parse — keeps the synthetic ratios instead.
    /// [`Self::script_scale_diagnostics`] reports that substitution so a
    /// fallback is never mistaken for a measured value.
    fn get_style_scale(&self, style: MathStyle) -> Fixed {
        match style {
            MathStyle::Display | MathStyle::Text => Fixed::ONE,
            MathStyle::Script => self.script_scale_ratio(MathConstant::ScriptPercentScaleDown),
            MathStyle::ScriptScript => self.script_scale_ratio(MathConstant::ScriptScriptPercentScaleDown),
        }
    }

    /// A script scale ratio as the font publishes it, else the synthetic one.
    fn script_scale_ratio(&self, constant: MathConstant) -> Fixed {
        if let Some(ratio) = self.font_system.font_math_constant("default", constant) {
            return ratio;
        }
        self.font_system.get_math_constant("default", constant).unwrap_or(Fixed::ONE)
    }

    /// A `MATH` constant in pixels, or `None` when nothing publishes it.
    ///
    /// The font's own `MathConstants` table is the truth source, and
    /// [`MathConstants::resolve`](vutex_text::math::MathConstants::resolve) says
    /// whether a number came from that table, from the crate's synthetic
    /// stand-in, or from nowhere at all. A constant nobody publishes returns
    /// `None`, so the caller drops the constraint instead of inventing a
    /// measurement. [`Self::script_scale_diagnostics`] reports every such
    /// substitution.
    fn math_constant(&self, constant: MathConstant, size: Fixed) -> Option<Fixed> {
        if let Some(em) = font_facts().and_then(|facts| facts.constants().resolve(constant).value()) {
            return Some(em * size);
        }
        self.font_system.font_math_constant("default", constant).map(|em| em * size)
    }

    /// Report every `MATH` value the layout consumes that the font does not
    /// publish.
    ///
    /// Script levels and the accent, limit and script gaps are still set, from
    /// the synthetic MATH values, so a font that failed to parse keeps laying
    /// out. This warning is the record of that substitution: a caller can tell a
    /// font-derived number from a fallback instead of both looking measured.
    pub fn script_scale_diagnostics(&self) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        let missing_ratios: Vec<&str> = [
            ("ScriptPercentScaleDown", MathConstant::ScriptPercentScaleDown),
            ("ScriptScriptPercentScaleDown", MathConstant::ScriptScriptPercentScaleDown),
        ]
        .into_iter()
        .filter(|(_, constant)| self.font_system.font_math_constant("default", *constant).is_none())
        .map(|(name, _)| name)
        .collect();
        if !missing_ratios.is_empty() {
            diagnostics.push(Diagnostic {
                code: "vutex.layout.math.script_scale_fallback".into(),
                severity: Severity::Warning,
                message: format!(
                    "the math font publishes no {} for family `default`, so script sizes use the synthetic MATH ratios",
                    missing_ratios.join("` or `")
                ),
            });
        }

        let missing_constants: Vec<&str> = [
            ("AccentBaseHeight", MathConstant::AccentBaseHeight),
            ("FlattenedAccentBaseHeight", MathConstant::FlattenedAccentBaseHeight),
            ("UpperLimitGapMin", MathConstant::UpperLimitGapMin),
            ("UpperLimitBaselineRiseMin", MathConstant::UpperLimitBaselineRiseMin),
            ("LowerLimitGapMin", MathConstant::LowerLimitGapMin),
            ("LowerLimitBaselineDropMin", MathConstant::LowerLimitBaselineDropMin),
            ("SuperscriptBaselineDropMax", MathConstant::SuperscriptBaselineDropMax),
            ("SubscriptBaselineDropMin", MathConstant::SubscriptBaselineDropMin),
        ]
        .into_iter()
        .filter(|(_, constant)| self.font_system.font_math_constant("default", *constant).is_none())
        .map(|(name, _)| name)
        .collect();
        if !missing_constants.is_empty() {
            diagnostics.push(Diagnostic {
                code: "vutex.layout.math.constant_fallback".into(),
                severity: Severity::Warning,
                message: format!(
                    "the math font publishes no {} for family `default`, so accent and limit gaps use the synthetic MATH values",
                    missing_constants.join("`, `")
                ),
            });
        }

        diagnostics
    }

    fn get_spacing_glue(&self, spacing: SpacingRule) -> Option<Glue> {
        match spacing {
            SpacingRule::Thin => Some(Glue { width: Fixed::from_f64(3.0), stretch: Fixed::ZERO, shrink: Fixed::ZERO }),
            SpacingRule::Medium => {
                Some(Glue { width: Fixed::from_f64(4.0), stretch: Fixed::from_f64(2.0), shrink: Fixed::ZERO })
            }
            SpacingRule::Thick => {
                Some(Glue { width: Fixed::from_f64(5.0), stretch: Fixed::from_f64(5.0), shrink: Fixed::ZERO })
            }
            _ => None,
        }
    }

    /// Pack children into a horizontal box, optionally justified to `target_width`.
    pub fn pack_hbox(&self, children: Vec<LayoutNode>, target_width: Option<Fixed>) -> LayoutNode {
        let mut width = Fixed::ZERO;
        let mut max_height = Fixed::ZERO;
        let mut max_depth = Fixed::ZERO;
        let mut total_stretch = Fixed::ZERO;
        let mut total_shrink = Fixed::ZERO;

        for child in &children {
            width = width + child.width();
            max_height = max_height.max(child.height());
            max_depth = max_depth.max(child.depth());

            if let LayoutNode::Glue(g) = child {
                total_stretch = total_stretch + g.stretch;
                total_shrink = total_shrink + g.shrink;
            }
        }

        let mut glue_set = 0.0;
        if let Some(target) = target_width {
            let diff = target - width;
            if diff > Fixed::ZERO {
                if total_stretch > Fixed::ZERO {
                    glue_set = diff.to_f64() / total_stretch.to_f64();
                }
            }
            else if diff < Fixed::ZERO {
                if total_shrink > Fixed::ZERO {
                    glue_set = diff.to_f64() / total_shrink.to_f64();
                }
            }
            width = target;
        }

        LayoutNode::HBox(Box::new(HBox { width, height: max_height, depth: max_depth, children, shift: Fixed::ZERO, glue_set }))
    }

    /// Pack children into a vertical box.
    pub fn pack_vbox(&self, children: Vec<LayoutNode>, _alignment: Alignment) -> LayoutNode {
        if children.is_empty() {
            return LayoutNode::VBox(Box::new(VBox {
                width: Fixed::ZERO,
                height: Fixed::ZERO,
                depth: Fixed::ZERO,
                children: Vec::new(),
                shift: Fixed::ZERO,
                glue_set: 0.0,
            }));
        }

        let mut width = Fixed::ZERO;

        let last_idx = children.len() - 1;
        let depth = children[last_idx].depth();

        let mut total_height = Fixed::ZERO;
        for (i, child) in children.iter().enumerate() {
            // Kerns, glue and penalties only advance vertically here: their
            // numeric value is vertical space, so it must not widen the box.
            if !matches!(child, LayoutNode::Kern(_) | LayoutNode::Glue(_) | LayoutNode::Penalty { .. }) {
                width = width.max(child.width());
            }
            if i < last_idx {
                total_height = total_height + child.height() + child.depth();
            }
            else {
                total_height = total_height + child.height();
            }
        }
        let height = total_height;

        LayoutNode::VBox(Box::new(VBox { width, height, depth, children, shift: Fixed::ZERO, glue_set: 0.0 }))
    }

    /// Breaks a sequence of nodes into multiple lines using the Knuth-Plass algorithm.
    ///
    /// Returns a vertical box with one horizontal box per line.
    pub fn break_lines(&self, nodes: Vec<LayoutNode>, line_widths: Vec<Fixed>, tolerance: f64) -> LayoutNode {
        if nodes.is_empty() {
            return self.pack_vbox(Vec::new(), Alignment::Left);
        }

        let kp_items: Vec<crate::math::knuth_plass::Item> = nodes.iter().map(|n| n.to_kp_item()).collect();
        let kp = crate::math::knuth_plass::KnuthPlass::new(line_widths.clone(), tolerance);
        let breaks = kp.find_breaks(&kp_items);

        let mut lines = Vec::new();
        let mut start = 0;

        for (i, &break_idx) in breaks.iter().enumerate() {
            let line_width = if i < line_widths.len() { line_widths[i] } else { *line_widths.last().unwrap_or(&Fixed::ZERO) };

            let line_nodes = nodes[start..=break_idx].to_vec();

            // If the break is at a penalty or glue, we might want to adjust the line nodes.
            // TeX usually removes glue at the beginning and end of lines.
            // For simplicity, we just pack them for now.

            lines.push(self.pack_hbox(line_nodes, Some(line_width)));
            start = break_idx + 1;
        }

        // Handle the last line (if any nodes left)
        if start < nodes.len() {
            let line_nodes = nodes[start..].to_vec();
            // The last line is usually not justified, so we pack it without a target width or with natural width.
            lines.push(self.pack_hbox(line_nodes, None));
        }

        self.pack_vbox(lines, Alignment::Left)
    }

    fn layout_fraction(
        &self,
        numerator: &SemanticNode,
        denominator: &SemanticNode,
        line_style: LineStyle,
        state: MathStyleState,
    ) -> MathResult<LayoutNode> {
        // TeX demotes fraction arguments exactly one style: display -> text,
        // text -> script, and both script styles -> scriptscript.  Keeping
        // script arguments in script style would let them outgrow the parent.
        // The numerator inherits `cramped`, the denominator is always cramped.
        let num_layout = self.layout_node_state(numerator, state.frac_num())?;
        let den_layout = self.layout_node_state(denominator, state.frac_den())?;

        let style = state.style;
        let family = "default";
        let style_scale = self.get_style_scale(style);
        let size = self.base_size * style_scale;

        let rule_thickness = match line_style {
            LineStyle::Solid => {
                self.font_system.get_math_constant(family, MathConstant::FractionRuleThickness).unwrap_or(Fixed::from_f64(0.06))
                    * size
            }
            LineStyle::None => Fixed::ZERO,
        };

        let num_shift = if style == MathStyle::Display {
            self.font_system
                .get_math_constant(family, MathConstant::FractionNumeratorDisplayStyleShiftUp)
                .unwrap_or(Fixed::from_f64(0.6))
                * size
        }
        else {
            self.font_system.get_math_constant(family, MathConstant::FractionNumeratorShiftUp).unwrap_or(Fixed::from_f64(0.35))
                * size
        };

        let den_shift = if style == MathStyle::Display {
            self.font_system
                .get_math_constant(family, MathConstant::FractionDenominatorDisplayStyleShiftDown)
                .unwrap_or(Fixed::from_f64(0.6))
                * size
        }
        else {
            self.font_system
                .get_math_constant(family, MathConstant::FractionDenominatorShiftDown)
                .unwrap_or(Fixed::from_f64(0.35))
                * size
        };

        let num_gap = if style == MathStyle::Display {
            self.font_system
                .get_math_constant(family, MathConstant::FractionNumDisplayStyleGapMin)
                .unwrap_or(Fixed::from_f64(0.1))
                * size
        }
        else {
            self.font_system.get_math_constant(family, MathConstant::FractionNumeratorGapMin).unwrap_or(Fixed::from_f64(0.05))
                * size
        };

        let den_gap = if style == MathStyle::Display {
            self.font_system
                .get_math_constant(family, MathConstant::FractionDenomDisplayStyleGapMin)
                .unwrap_or(Fixed::from_f64(0.1))
                * size
        }
        else {
            self.font_system.get_math_constant(family, MathConstant::FractionDenominatorGapMin).unwrap_or(Fixed::from_f64(0.05))
                * size
        };

        let axis_height =
            self.font_system.get_math_constant(family, MathConstant::AxisHeight).unwrap_or(Fixed::from_f64(0.25)) * size;

        let num_width = num_layout.width();
        let num_depth = num_layout.depth();
        let den_width = den_layout.width();
        let den_height = den_layout.height();

        let width = num_width.max(den_width) + Fixed::from_f64(0.2) * size;

        let mut v_children = Vec::new();

        // Centering numerator
        let num_hbox = self
            .pack_hbox(vec![LayoutNode::Kern((width - num_layout.width()) / Fixed::from_f64(2.0)), num_layout], Some(width));
        v_children.push(num_hbox);

        // Gap between num and rule
        let actual_num_shift = num_shift.max(axis_height + rule_thickness / 2.0 + num_gap + num_depth);
        v_children.push(LayoutNode::Kern(actual_num_shift - axis_height - rule_thickness / 2.0 - num_depth));

        if rule_thickness > Fixed::ZERO {
            v_children.push(LayoutNode::Rule { width, height: rule_thickness, depth: Fixed::ZERO });
        }

        // Gap between rule and den
        //
        // `actual_den_shift` is the distance from the fraction's baseline down
        // to the denominator baseline, and the rule centre sits `AxisHeight`
        // above that baseline, so the clearance below the rule is
        // `actual_den_shift + axis - rule/2 - den_height`.  Requiring it to stay
        // at least `den_gap` gives the minimum below and the kern that follows.
        // Both sides of the rule must be measured from the axis: taking
        // `axis - rule/2` here inverts the sign and leaves an extra half axis
        // of empty space under the rule.
        let den_min_shift = rule_thickness / Fixed::from_f64(2.0) + den_gap + den_height - axis_height;
        let actual_den_shift = den_shift.max(den_min_shift);
        v_children.push(LayoutNode::Kern(actual_den_shift + axis_height - rule_thickness / Fixed::from_f64(2.0) - den_height));

        let den_hbox = self
            .pack_hbox(vec![LayoutNode::Kern((width - den_layout.width()) / Fixed::from_f64(2.0)), den_layout], Some(width));
        v_children.push(den_hbox);

        let mut vbox_node = self.pack_vbox(v_children, Alignment::Center);
        if let LayoutNode::VBox(ref mut v) = vbox_node {
            v.shift = Fixed::ZERO;
        }

        // The vertical list is built downwards from the numerator and its last
        // item is the denominator, so the list's own reference point is the
        // denominator baseline.  Lowering the whole list by `actual_den_shift`
        // puts the fraction baseline back on the math axis: the rule centre then
        // lands exactly `AxisHeight` above it, and `height`/`depth` fold the
        // shift so the box reports its real extents around the axis.
        let mut wrapper = self.pack_hbox(vec![vbox_node], None);
        if let LayoutNode::HBox(ref mut h) = wrapper {
            h.shift = actual_den_shift;
        }

        Ok(wrapper)
    }

    fn layout_radical(
        &self,
        degree: Option<&SemanticNode>,
        radicand: &SemanticNode,
        state: MathStyleState,
    ) -> MathResult<LayoutNode> {
        // A radical's radicand is cramped, so any superscript inside it takes
        // `SuperscriptShiftUpCramped`.
        let radicand_layout = self.layout_node_state(radicand, state.cramp())?;

        let style = state.style;
        let family = "default";
        let style_scale = self.get_style_scale(style);
        let size = self.base_size * style_scale;

        let rule_thickness =
            self.font_system.get_math_constant(family, MathConstant::RadicalRuleThickness).unwrap_or(Fixed::from_f64(0.06))
                * size;
        let vertical_gap = if style == MathStyle::Display {
            self.font_system
                .get_math_constant(family, MathConstant::RadicalDisplayStyleVerticalGap)
                .unwrap_or(Fixed::from_f64(0.1))
                * size
        }
        else {
            self.font_system.get_math_constant(family, MathConstant::RadicalVerticalGap).unwrap_or(Fixed::from_f64(0.05)) * size
        };
        // TeX's `RadicalVerticalGap` is the font's own clearance between the
        // rule and the radicand's ink; do not inflate it, the MATH table already
        // publishes the value that reads correctly with its own radical design.
        // Height of the radical bar above the radicand baseline. The sign is
        // raised so its own top bar lands exactly on this line and the two
        // strokes read as one continuous rule.
        let bar_top = radicand_layout.height() + vertical_gap + rule_thickness;
        // The sign reaches this far below the baseline so it hugs the radicand.
        let sign_bottom = radicand_layout.depth() + vertical_gap;

        // The font grows the radical sign with `MathVariants`: a prepared design
        // covering the bar, or a connector assembly beyond the tallest design. A
        // sign drawn from the base outline is never scaled — that would be an
        // invented design, not a font fact.
        let radical_key = GlyphKey::from_char('√', Some("default".to_string()), FontStyle::Normal);
        let radical_symbol = self.radical_sign(&radical_key, bar_top + sign_bottom, size)?;

        // Raise the sign so its own top bar lands on the bar line. The shift is
        // an extra render offset that `LayoutNode::height` subtracts again, so
        // the wrapper reports the raised ascent without counting it twice.
        let mut radical_box = self.pack_hbox(vec![radical_symbol], None);
        if let LayoutNode::HBox(h) = &mut radical_box {
            let rise = bar_top - h.height;
            h.shift = Fixed::ZERO - rise;
        }

        let mut children = Vec::new();

        if let Some(deg_node) = degree {
            // TeXbook rule 11 and the MATH table agree on three offsets: a kern
            // before the index, a (negative) kern after it that tucks the surd
            // under the index, and a bottom raise taken as a percentage of the
            // radical's own ascent above the radicand.
            let deg_layout = self.layout_node(deg_node, MathStyle::ScriptScript)?;
            let kern_before = self
                .font_system
                .get_math_constant(family, MathConstant::RadicalKernBeforeDegree)
                .unwrap_or(Fixed::from_f64(0.2))
                * size;
            let kern_after = self
                .font_system
                .get_math_constant(family, MathConstant::RadicalKernAfterDegree)
                .unwrap_or(Fixed::from_f64(-0.2))
                * size;
            let raise_percent = self
                .font_system
                .get_math_constant(family, MathConstant::RadicalDegreeBottomRaisePercent)
                .unwrap_or(Fixed::from_f64(0.6));
            let raise = raise_percent * (bar_top - radicand_layout.depth());

            let mut deg_vbox = Vec::new();
            deg_vbox.push(deg_layout);
            deg_vbox.push(LayoutNode::Kern(raise));
            children.push(LayoutNode::Kern(kern_before));
            children.push(self.pack_vbox(deg_vbox, Alignment::Left));
            children.push(LayoutNode::Kern(kern_after));
        }

        children.push(radical_box);

        let mut radicand_vbox = Vec::new();
        radicand_vbox.push(LayoutNode::Rule { width: radicand_layout.width(), height: rule_thickness, depth: Fixed::ZERO });
        radicand_vbox.push(LayoutNode::Kern(vertical_gap));
        radicand_vbox.push(radicand_layout);

        let packed_radicand = self.pack_vbox(radicand_vbox, Alignment::Left);
        children.push(packed_radicand);

        Ok(self.pack_hbox(children, None))
    }

    /// The radical sign the font would draw for an extent of `target`.
    ///
    /// `MathVariants` publishes the radical as a construction, so the sign must
    /// be the prepared design the font's own size search picks. The metrics
    /// cache only carries the base sign's outline, so a prepared design without
    /// an outline there is not paintable yet and the base sign is used as-is:
    /// stretching the base outline would invent a design the font never drew.
    fn radical_sign(&self, key: &GlyphKey, target: Fixed, size: Fixed) -> MathResult<LayoutNode> {
        if let Some(facts) = font_facts() {
            if let Some(glyph) = font_facts_glyph(key) {
                if let Some(StretchPlan::Variant(variant)) =
                    facts.variants().stretch(glyph, StretchAxis::Vertical, target / size)
                {
                    let variant_key = GlyphKey { glyph_id: Some(variant.glyph.get()), ..key.clone() };
                    if self.font_system.get_glyph_path(&variant_key).is_ok() {
                        return self.layout_scaled_symbol(&variant_key, size);
                    }
                }
            }
        }
        self.layout_scaled_symbol(key, size)
    }

    fn layout_limits(
        &self,
        base: &SemanticNode,
        sub: Option<&SemanticNode>,
        sup: Option<&SemanticNode>,
        state: MathStyleState,
    ) -> MathResult<LayoutNode> {
        // Upper and lower limits are one style smaller than the operator, so a
        // display/text operator takes script limits and a script operator takes
        // scriptscript limits.  An upper limit inherits `cramped`, a lower one
        // is forced cramped, exactly like the scripts they mirror.
        let style = state.style;

        let style_scale = self.get_style_scale(style);
        let size = self.base_size * style_scale;

        let base_layout = self.layout_node_state(base, state)?;
        let axis_height = self.math_constant(MathConstant::AxisHeight, size).unwrap_or(Fixed::ZERO);
        let base_shift = axis_height - (base_layout.height() - base_layout.depth()) / Fixed::from_f64(2.0);

        let mut wrapper = self.pack_hbox(vec![base_layout], None);
        if let LayoutNode::HBox(ref mut h) = wrapper {
            h.shift = Fixed::ZERO - base_shift; // Negative shift moves UP
        }
        let base_layout = wrapper;

        // We need the widths of sup and sub to calculate the total width
        let mut sup_layout = None;
        if let Some(s) = sup {
            sup_layout = Some(self.layout_node_state(s, state.sup())?);
        }
        let mut sub_layout = None;
        if let Some(s) = sub {
            sub_layout = Some(self.layout_node_state(s, state.sub())?);
        }

        let total_width = base_layout
            .width()
            .max(sup_layout.as_ref().map(|n| n.width()).unwrap_or(Fixed::ZERO))
            .max(sub_layout.as_ref().map(|n| n.width()).unwrap_or(Fixed::ZERO));

        let mut v_children = Vec::new();

        if let Some(sup_l) = sup_layout {
            let gap = self.math_constant(MathConstant::UpperLimitGapMin, size).unwrap_or(Fixed::ZERO);
            let rise = self.math_constant(MathConstant::UpperLimitBaselineRiseMin, size).unwrap_or(Fixed::ZERO);
            let kern = gap.max(rise - base_layout.height() - sup_l.depth());

            let sup_hbox = self.pack_hbox(
                vec![LayoutNode::Kern((total_width - sup_l.width()) / Fixed::from_f64(2.0)), sup_l],
                Some(total_width),
            );
            v_children.push(sup_hbox);
            v_children.push(LayoutNode::Kern(kern));
        }

        let base_depth = base_layout.depth();
        let base_hbox = self.pack_hbox(
            vec![LayoutNode::Kern((total_width - base_layout.width()) / Fixed::from_f64(2.0)), base_layout],
            Some(total_width),
        );
        v_children.push(base_hbox);

        let mut baseline_drop = Fixed::ZERO;
        let mut has_sub = false;

        if let Some(sub_l) = sub_layout {
            let gap = self.math_constant(MathConstant::LowerLimitGapMin, size).unwrap_or(Fixed::ZERO);
            let drop = self.math_constant(MathConstant::LowerLimitBaselineDropMin, size).unwrap_or(Fixed::ZERO);
            let sub_height = sub_l.height();
            has_sub = true;

            // `LowerLimitGapMin` is the clearance below the operator's own
            // bottom, while `LowerLimitBaselineDropMin` measures baseline to
            // baseline across the subscript's height. Taking the max of both
            // requirements keeps the gap rule from being applied on top of an
            // already sufficient baseline drop.
            baseline_drop = (base_depth + gap + sub_height).max(drop);

            v_children.push(LayoutNode::Kern(gap));
            let sub_hbox = self.pack_hbox(
                vec![LayoutNode::Kern((total_width - sub_l.width()) / Fixed::from_f64(2.0)), sub_l],
                Some(total_width),
            );
            v_children.push(sub_hbox);
        }

        let vbox_node = self.pack_vbox(v_children, Alignment::Center);

        if has_sub {
            let shift = baseline_drop;

            let mut wrapper = self.pack_hbox(vec![vbox_node], None);
            if let LayoutNode::HBox(ref mut h) = wrapper {
                h.shift = shift;
            }
            Ok(wrapper)
        }
        else {
            Ok(vbox_node)
        }
    }

    /// Attach `sub` / `sup` to `base` in `style`.
    ///
    /// `class` and `limits` are resolved by the caller from the noad being laid
    /// out: `\limits` / `\nolimits` belongs to the atom it was authored after,
    /// so it arrives as an argument instead of being stashed on the engine and
    /// consumed by whichever large operator happens to be lowered next.
    fn layout_subsup(
        &self,
        base: &SemanticNode,
        sub: Option<&SemanticNode>,
        sup: Option<&SemanticNode>,
        state: MathStyleState,
        class: OperatorClass,
        limits: LimitsPolicy,
    ) -> MathResult<LayoutNode> {
        let style = state.style;
        if class.places_limits(limits, style) {
            return self.layout_limits(base, sub, sup, state);
        }

        let base_layout = self.layout_node_state(base, state)?;

        let style_scale = self.get_style_scale(style);
        let size = self.base_size * style_scale;

        let mut script_vbox_children = Vec::new();
        let mut calculated_sub_shift = Fixed::ZERO;
        let mut sup_depth = Fixed::ZERO;
        // How far each script's ink reaches away from the base baseline. TeX
        // feeds that extent to the base glyph's corner kern, so it is recorded
        // while the shifts are being decided.
        let mut sup_correction_height = Fixed::ZERO;
        let mut sub_correction_depth = Fixed::ZERO;

        // Script minima depend on the kind of nucleus. The MATH table publishes
        // baseline drop limits for a base that is a box or an extended shape,
        // while a plain glyph keeps its own shift minima. A large operator
        // instead follows the `op` atom rules `layout_limits` and the shift
        // minima below already encode, so it is not treated as extended here.
        let base_key = Self::first_glyph_key(base);
        let extended_base = match base {
            SemanticNode::Symbol { glyph_key, .. } => {
                !class.is_large_operator()
                    && match (font_facts(), font_facts_glyph(glyph_key)) {
                        (Some(facts), Some(glyph)) => facts.is_extended_shape(glyph),
                        _ => false,
                    }
            }
            SemanticNode::Text { .. } => false,
            _ => true,
        };

        if let Some(sup_node) = sup {
            let sup_layout = self.layout_node_state(sup_node, state.sup())?;
            // A cramped base takes the font's cramped minimum instead of the
            // regular one.  `SuperscriptBottomMin` still wins over both when the
            // nucleus is tall enough.
            let sup_constant =
                if state.cramped { MathConstant::SuperscriptShiftUpCramped } else { MathConstant::SuperscriptShiftUp };
            let sup_shift = self.math_constant(sup_constant, size).unwrap_or(Fixed::ZERO);
            let sup_bottom_min = self.math_constant(MathConstant::SuperscriptBottomMin, size).unwrap_or(Fixed::ZERO);

            let mut calculated_sup_shift = sup_shift.max(base_layout.height() + sup_bottom_min);
            if extended_base {
                // `SuperscriptBaselineDropMax` caps how far a superscript's
                // baseline may drop below the top of a box nucleus.
                let drop_max = self.math_constant(MathConstant::SuperscriptBaselineDropMax, size).unwrap_or(Fixed::ZERO);
                calculated_sup_shift = calculated_sup_shift.max(base_layout.height() - drop_max);
            }

            sup_depth = sup_layout.depth();
            sup_correction_height = calculated_sup_shift + sup_layout.height();
            script_vbox_children.push(sup_layout);
            // This kern will be between Sup and the baseline (or Sub)
            script_vbox_children.push(LayoutNode::Kern(calculated_sup_shift));
        }

        if let Some(sub_node) = sub {
            let sub_layout = self.layout_node_state(sub_node, state.sub())?;
            let sub_shift = self.math_constant(MathConstant::SubscriptShiftDown, size).unwrap_or(Fixed::ZERO);
            let sub_top_max = self.math_constant(MathConstant::SubscriptTopMax, size).unwrap_or(Fixed::ZERO);

            calculated_sub_shift = sub_shift.max(sub_layout.height() - sub_top_max);
            if extended_base {
                // `SubscriptBaselineDropMin` keeps a subscript below the ink
                // bottom of a box nucleus instead of letting it lift into it.
                let drop_min = self.math_constant(MathConstant::SubscriptBaselineDropMin, size).unwrap_or(Fixed::ZERO);
                calculated_sub_shift = calculated_sub_shift.max(base_layout.depth() + drop_min);
            }
            sub_correction_depth = calculated_sub_shift + sub_layout.depth();

            // If we already have a superscript, we need to adjust the kern between them
            if script_vbox_children.len() == 2 {
                if let LayoutNode::Kern(sup_kern) = script_vbox_children.pop().unwrap() {
                    let subsup_gap = self.math_constant(MathConstant::SubSuperscriptGapMin, size).unwrap_or(Fixed::ZERO);

                    let sub_height = sub_layout.height();
                    let baseline_distance = (sup_kern + calculated_sub_shift).max(sup_depth + subsup_gap + sub_height);

                    // A vertical box advances a child by its full line height, so
                    // the kern between the two scripts is the baseline distance
                    // less the superscript's depth and the subscript's height.
                    // Pushing the baseline distance itself would add both of them
                    // again and lift the superscript by the subscript's height.
                    script_vbox_children.push(LayoutNode::Kern(baseline_distance - sup_depth - sub_height));
                }
            }
            else {
                // Only subscript, push the shift kern before the subscript
                script_vbox_children.push(LayoutNode::Kern(calculated_sub_shift));
            }

            script_vbox_children.push(sub_layout);
        }

        let scripts = self.pack_vbox(script_vbox_children, Alignment::Left);

        // Adjust the shift of the scripts VBox so it aligns correctly with the base.
        let final_scripts = match scripts {
            LayoutNode::VBox(mut v) => {
                if sub.is_some() && sup.is_some() {
                    // Baseline of VBox is Sub baseline.
                    // We want the base baseline to be calculated_sub_shift above Sub baseline.
                    // Wait, if we adjusted total_dist, the actual distance between baselines might be different.
                    // Actually, if we use pack_vbox, the baseline is the baseline of the last child (Sub).
                    // So we shift the whole VBox down by calculated_sub_shift?
                    // No, shifting down means positive shift in our VBox logic (translate(x + shift, y)).
                    // Wait, VBox shift is on X axis in render_layout_node?
                    // Let's check render_layout_node for VBox.
                    // LayoutNode::VBox(vbox) => {
                    //     backend.start_group(Some(&format!("translate({}, {})", x + vbox.shift.to_f64(), y)))?;
                    // HBox shift is on Y axis.
                    // LayoutNode::HBox(hbox) => {
                    //     backend.start_group(Some(&format!("translate({}, {})", x, y + hbox.shift.to_f64())))?;

                    // Ah! VBox shift is X-offset, HBox shift is Y-offset.
                    // So for scripts, we need to wrap it in an HBox and shift that HBox.
                    v.shift = Fixed::ZERO;
                    let node = LayoutNode::VBox(v);
                    let mut wrapper = self.pack_hbox(vec![node], None);
                    if let LayoutNode::HBox(ref mut h) = wrapper {
                        if sub.is_some() {
                            // Baseline of VBox is Sub baseline.
                            // We want base baseline to be calculated_sub_shift above Sub baseline.
                            // So we shift the VBox down by calculated_sub_shift.
                            h.shift = calculated_sub_shift;
                        }
                        else if sup.is_some() {
                            // Baseline is already correct for superscript only.
                            h.shift = Fixed::ZERO;
                        }
                    }
                    wrapper
                }
                else if sub.is_some() {
                    v.shift = Fixed::ZERO;
                    let node = LayoutNode::VBox(v);
                    let mut wrapper = self.pack_hbox(vec![node], None);
                    if let LayoutNode::HBox(ref mut h) = wrapper {
                        h.shift = calculated_sub_shift;
                    }
                    wrapper
                }
                else if sup.is_some() {
                    v.shift = Fixed::ZERO;
                    let node = LayoutNode::VBox(v);
                    let mut wrapper = self.pack_hbox(vec![node], None);
                    if let LayoutNode::HBox(ref mut h) = wrapper {
                        // The kern above the last child is measured from the box's
                        // top, so the superscript's own depth is already inside the
                        // rise. Shift it back out to land on `SuperscriptShiftUp`.
                        h.shift = Fixed::ZERO - sup_depth;
                    }
                    wrapper
                }
                else {
                    LayoutNode::VBox(v)
                }
            }
            node => node,
        };

        let mut children = Vec::new();
        children.push(base_layout);
        // A large operator keeps TeX's `\nolimits` placement when its scripts
        // sit beside it: the scripts hang off the operator's advance, not off
        // its italic correction. `\int` carries a third of an em of italic
        // correction that only exists to keep a following atom clear of the
        // slanted tail, so folding it in would push the scripts far right. The
        // gap is the font's own script spacing instead. Limits above and below
        // the operator are built by `layout_limits` and never reach this kern.
        if class.is_large_operator() {
            let script_space = self.math_constant(MathConstant::SpaceAfterScript, size).unwrap_or(Fixed::ZERO);
            // The `MATH` table publishes the italic correction the operator
            // actually carries. The metrics cache answers zero for a glyph whose
            // extents came from a lookup that never saw it.
            let italic_correction = base_key
                .as_ref()
                .and_then(|key| self.facts_italic_correction(key))
                .map(|em| em * size)
                .unwrap_or_else(|| children[0].italic_correction());
            children.push(LayoutNode::Kern(script_space - italic_correction));
        }

        // TeX kerns the script against the base glyph's facing corner: a
        // superscript reads the top-right corner at its own extent above the
        // baseline, a lone subscript the bottom-right corner at its depth. A
        // font that publishes no such corner attaches the script at the advance
        // with no kern.
        let script_kern = base_key.as_ref().and_then(|key| {
            let (corner, correction_height) = match (sup.is_some(), sub.is_some()) {
                (true, _) => (KernCorner::TopRight, sup_correction_height),
                (false, true) => (KernCorner::BottomRight, sub_correction_depth),
                (false, false) => return None,
            };
            self.math_kern(key, corner, correction_height, size)
        });
        if let Some(kern) = script_kern {
            if kern != Fixed::ZERO {
                children.push(LayoutNode::Kern(kern));
            }
        }

        children.push(final_scripts);

        Ok(self.pack_hbox(children, None))
    }

    /// Lay out one AMS environment block.
    ///
    /// Each kind carries its own row rule, and the rule comes from the
    /// environment rather than from the author's cells:
    ///
    /// - `aligned`, `alignedat` and `align` alternate a right-aligned and a
    ///   left-aligned column at every `&` anchor, exactly like `alignedat{n}`
    ///   pins its grid to `2n` columns.
    /// - `split` rows are fragments of one equation that meet at a single
    ///   alignment point, so they use the same right/left anchor grid with no
    ///   two-row cap.
    /// - `equation` and `gather` have no anchors and centre every row over the
    ///   widest one.
    /// - `multline` flushes its first row left, its last row right and centres
    ///   the rows between.
    ///
    /// Row numbering stays an IR intent: VuTeX keeps the authored `Numbering`
    /// and assigns no number itself.
    fn layout_environment(&self, environment: &MathEnvironment, state: MathStyleState) -> MathResult<LayoutNode> {
        // `align`, `equation`, `gather` and `multline` pin display style so an
        // enclosing `\scriptstyle` cannot demote their rows.  `aligned`,
        // `alignedat` and `split` follow the surrounding style.
        let state =
            if environment.kind.is_display_level() { MathStyleState { style: MathStyle::Display, ..state } } else { state };
        if environment.rows.is_empty() {
            return Ok(LayoutNode::Kern(Fixed::ZERO));
        }

        let style_scale = self.get_style_scale(state.style);
        let size = self.base_size * style_scale;
        let row_spacing = environment.row_spacing * size;
        let col_spacing = environment.col_spacing * size;
        let anchors = environment.kind.anchors();

        // `alignedat{n}` presents exactly its `2n` declared columns, so a short
        // row leaves the trailing columns empty instead of shrinking the grid.
        // Every other kind takes the widest authored row.  A body that types
        // more columns than it declares keeps them rather than dropping cells.
        let columns = environment.columns().max(environment.rows.iter().map(|row| row.cells.len()).max().unwrap_or(0));
        if columns == 0 {
            return Ok(LayoutNode::Kern(Fixed::ZERO));
        }

        // Layout every cell once.  `alignedat` padding reaches layout as
        // missing cells, so an absent cell is an empty kern rather than a
        // narrower row.
        let mut cell_layouts: Vec<Vec<LayoutNode>> = Vec::with_capacity(environment.rows.len());
        for row in &environment.rows {
            let mut cells = Vec::with_capacity(columns);
            for column in 0..columns {
                match row.cells.get(column) {
                    Some(cell) => cells.push(self.layout_node_state(cell, state)?),
                    None => cells.push(LayoutNode::Kern(Fixed::ZERO)),
                }
            }
            cell_layouts.push(cells);
        }

        let mut row_boxes: Vec<LayoutNode> = Vec::with_capacity(environment.rows.len());
        match anchors {
            // `aligned`, `alignedat`, `align` and `split`: every `&` closes a
            // right-aligned column and opens a left-aligned one.  A relation
            // that opens the left column has no left neighbour inside its own
            // cell, so the grid supplies the thick muskip TeX puts on both sides
            // of a relation.  These grids carry no intercolumn gutter, so
            // `col_spacing` is the environment's own value (zero for them).
            AlignmentAnchors::ColumnPairs | AlignmentAnchors::SingleEquation => {
                let mut col_widths = vec![Fixed::ZERO; columns];
                for cells in &cell_layouts {
                    for (column, cell) in cells.iter().enumerate() {
                        col_widths[column] = col_widths[column].max(cell.width());
                    }
                }
                for (row_index, cells) in cell_layouts.into_iter().enumerate() {
                    let mut children = Vec::new();
                    for (column, cell) in cells.into_iter().enumerate() {
                        let diff = col_widths[column] - cell.width();
                        let cell_box = if column % 2 == 0 {
                            self.pack_hbox(vec![LayoutNode::Kern(diff), cell], Some(col_widths[column]))
                        }
                        else {
                            self.pack_hbox(vec![cell, LayoutNode::Kern(diff)], Some(col_widths[column]))
                        };
                        if column > 0
                            && environment.rows[row_index].cells.get(column).and_then(Self::leading_role)
                                == Some(SymbolRole::Relation)
                        {
                            children.push(LayoutNode::Kern(Fixed::from_f64(THICK_MUSIP_EM) * size));
                        }
                        children.push(cell_box);
                        if column + 1 < columns && col_spacing > Fixed::ZERO {
                            children.push(LayoutNode::Kern(col_spacing));
                        }
                    }
                    row_boxes.push(self.pack_hbox(children, None));
                }
            }
            // `equation` and `gather`: no anchors, so every row is one centred
            // line.  A stray `&` still separates cells, and the gutter is all
            // that holds them together.
            AlignmentAnchors::Centered => {
                for cells in cell_layouts {
                    let mut children = Vec::new();
                    for (column, cell) in cells.into_iter().enumerate() {
                        if column > 0 && col_spacing > Fixed::ZERO {
                            children.push(LayoutNode::Kern(col_spacing));
                        }
                        children.push(cell);
                    }
                    row_boxes.push(self.pack_hbox(children, None));
                }
            }
            // `multline`: the first row starts at the left edge, the last ends
            // at the right edge and the rows between are centred.  A single row
            // cannot be both edges, so it stays centred.
            AlignmentAnchors::Spread => {
                for cells in cell_layouts {
                    let mut children = Vec::new();
                    for (column, cell) in cells.into_iter().enumerate() {
                        if column > 0 && col_spacing > Fixed::ZERO {
                            children.push(LayoutNode::Kern(col_spacing));
                        }
                        children.push(cell);
                    }
                    row_boxes.push(self.pack_hbox(children, None));
                }
            }
        }

        // A `Centered` or `Spread` block pads each row to the widest one, so the
        // rows share one box width and the `VBox` can stack them at one left
        // edge.  Anchor grids already pad every cell to its column, so their
        // rows are uniform without this pass.
        if !matches!(anchors, AlignmentAnchors::ColumnPairs | AlignmentAnchors::SingleEquation) {
            let widest = row_boxes.iter().map(LayoutNode::width).max().unwrap_or(Fixed::ZERO);
            let row_count = row_boxes.len();
            let last = row_count - 1;
            for (index, row) in row_boxes.iter_mut().enumerate() {
                let diff = widest - row.width();
                let half = diff / Fixed::from_f64(2.0);
                let (left, right) = match anchors {
                    AlignmentAnchors::Spread if row_count > 1 && index == 0 => (Fixed::ZERO, diff),
                    AlignmentAnchors::Spread if row_count > 1 && index == last => (diff, Fixed::ZERO),
                    _ => (half, diff - half),
                };
                let padded = std::mem::replace(row, LayoutNode::Kern(Fixed::ZERO));
                *row = self.pack_hbox(vec![LayoutNode::Kern(left), padded, LayoutNode::Kern(right)], None);
            }
        }

        let mut stacked = Vec::new();
        for (index, row) in row_boxes.into_iter().enumerate() {
            if index > 0 && row_spacing > Fixed::ZERO {
                stacked.push(LayoutNode::Kern(row_spacing));
            }
            stacked.push(row);
        }

        let block = self.pack_vbox(stacked, Alignment::Center);
        Ok(self.centre_block_on_axis(block, size))
    }

    /// Centre a stacked block on the `MATH` axis.
    ///
    /// The `VBox` measures the block from its top edge down to its bottom edge,
    /// so the baseline shift that puts the block's middle on the axis is
    /// `(height - depth) / 2 - axis`.  A font that publishes no `AxisHeight`
    /// leaves the block on its own baseline instead of inventing one.
    fn centre_block_on_axis(&self, block: LayoutNode, size: Fixed) -> LayoutNode {
        let axis_height = self.math_constant(MathConstant::AxisHeight, size).unwrap_or(Fixed::ZERO);
        let (height, depth) = match &block {
            LayoutNode::VBox(v) => (v.height, v.depth),
            _ => (Fixed::ZERO, Fixed::ZERO),
        };
        let shift = (height - depth) / Fixed::from_f64(2.0) - axis_height;
        let mut wrapper = self.pack_hbox(vec![block], None);
        if let LayoutNode::HBox(ref mut h) = wrapper {
            // `LayoutNode::height` / `depth` already fold `shift` back into the
            // extents, exactly like `center_delimiter` relies on.  Writing the
            // shifted values into `height` / `depth` as well would apply the
            // shift twice and invert the box, so only the shift is stored.
            h.shift = shift;
        }
        wrapper
    }

    fn layout_matrix(
        &self,
        rows: &[&[SemanticNode]],
        row_spacing: Fixed,
        col_spacing: Fixed,
        column_alignment: &[Alignment],
        state: MathStyleState,
    ) -> MathResult<LayoutNode> {
        if rows.is_empty() {
            return Ok(LayoutNode::Kern(Fixed::ZERO));
        }

        let num_rows = rows.len();
        let num_cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);

        if num_cols == 0 {
            return Ok(LayoutNode::Kern(Fixed::ZERO));
        }

        let style_scale = self.get_style_scale(state.style);
        let size = self.base_size * style_scale;

        let row_spacing = row_spacing * size;
        let col_spacing = col_spacing * size;

        // 1. Layout all cells and find column widths and row heights/depths
        let mut cell_layouts = vec![vec![None; num_cols]; num_rows];
        let mut col_widths = vec![Fixed::ZERO; num_cols];

        for r in 0..num_rows {
            for c in 0..rows[r].len() {
                let layout = self.layout_node_state(&rows[r][c], state)?;
                col_widths[c] = col_widths[c].max(layout.width());
                cell_layouts[r][c] = Some(layout);
            }
        }

        // 2. Pack cells into rows
        //
        // `aligned`, `alignedat{n}`, `align`, `align*` and `split` grids all
        // alternate a right-aligned and a left-aligned column.  A relation such
        // as `=` normally opens the left column, where it has no left neighbour
        // inside its own cell, so the grid has to supply the thick muskip TeX
        // puts on both sides of a relation.  Without it the relation keeps only
        // its right-hand space and the equation reads lopsided.
        //
        // `alignedat{n}` fixes the grid to exactly `n` column pairs by padding
        // short rows with empty cells (parser responsibility), so `layout_matrix`
        // only sees a uniform `column_alignment` array.  `split` is just one
        // equation split over two rows — it shares the rl geometry of `aligned`
        // and is consumed identically by the layout path.
        let rl_grid = matches!(column_alignment.first(), Some(Alignment::Right));
        let mut packed_rows = Vec::new();
        for r in 0..num_rows {
            let mut row_children = Vec::new();
            for c in 0..num_cols {
                if let Some(cell) = cell_layouts[r][c].take() {
                    let diff = col_widths[c] - cell.width();
                    let alignment = column_alignment.get(c).copied().unwrap_or(Alignment::Center);
                    let cell_node = match alignment {
                        Alignment::Left => self.pack_hbox(vec![cell, LayoutNode::Kern(diff)], Some(col_widths[c])),
                        Alignment::Center => {
                            let half = diff / Fixed::from_f64(2.0);
                            self.pack_hbox(
                                vec![LayoutNode::Kern(half), cell, LayoutNode::Kern(diff - half)],
                                Some(col_widths[c]),
                            )
                        }
                        Alignment::Right => self.pack_hbox(vec![LayoutNode::Kern(diff), cell], Some(col_widths[c])),
                    };
                    if c > 0 && rl_grid && Self::leading_role(&rows[r][c]) == Some(SymbolRole::Relation) {
                        row_children.push(LayoutNode::Kern(Fixed::from_f64(THICK_MUSIP_EM) * size));
                    }
                    row_children.push(cell_node);
                }
                else {
                    row_children.push(LayoutNode::Kern(col_widths[c]));
                }

                if c + 1 < num_cols {
                    row_children.push(LayoutNode::Kern(col_spacing));
                }
            }
            packed_rows.push(self.pack_hbox(row_children, None));

            if r + 1 < num_rows {
                packed_rows.push(LayoutNode::Kern(row_spacing));
            }
        }

        let matrix_vbox = self.pack_vbox(packed_rows, Alignment::Center);
        Ok(self.centre_block_on_axis(matrix_vbox, size))
    }

    fn layout_delimited(
        &self,
        left: Option<&GlyphKey>,
        right: Option<&GlyphKey>,
        content: &SemanticNode,
        state: MathStyleState,
    ) -> MathResult<LayoutNode> {
        let content_layout = self.layout_node_state(content, state)?;
        self.layout_fence(left, right, content_layout, state)
    }

    /// Size a `\left…\right` pair around an already-laid-out body.
    ///
    /// Both delimiters are built from `content_layout` alone, so the pair keeps
    /// the same extents whether it is the whole formula or sits after another
    /// atom.  The one-noad `Delimited` path and the projected `Open` / `Close`
    /// fence pair both route here for exactly that reason.
    fn layout_fence(
        &self,
        left: Option<&GlyphKey>,
        right: Option<&GlyphKey>,
        content_layout: LayoutNode,
        state: MathStyleState,
    ) -> MathResult<LayoutNode> {
        let style = state.style;
        let style_scale = self.get_style_scale(style);
        let size = self.base_size * style_scale;
        let family = "default";
        let axis_height =
            self.font_system.get_math_constant(family, MathConstant::AxisHeight).unwrap_or(Fixed::from_f64(0.25)) * size;

        // TeX sizes a delimiter from how far the content reaches away from the
        // math axis: the glyph must cover the taller of the two sides, mirrored
        // around the axis. Summing `height + depth` instead under-sizes every
        // asymmetric body such as a fraction, whose extents sit mostly below the
        // axis once the denominator is packed.
        let delta = (content_layout.height() - axis_height).max(content_layout.depth() + axis_height);
        let target_height = (delta * Fixed::from_f64(2.0)).max(size);

        let mut children = Vec::new();

        if let Some(left_key) = left {
            children.push(self.construct_delimiter(left_key, target_height, size, axis_height)?);
        }

        children.push(content_layout);

        if let Some(right_key) = right {
            children.push(self.construct_delimiter(right_key, target_height, size, axis_height)?);
        }

        Ok(self.pack_hbox(children, None))
    }

    fn construct_delimiter(
        &self,
        key: &GlyphKey,
        target_height: Fixed,
        size: Fixed,
        axis_height: Fixed,
    ) -> MathResult<LayoutNode> {
        let construction = self.font_system.get_delimiter_construction(key)?;

        // 1. The font's own size search. `MathVariants` decides which prepared
        // design covers the target and how an assembly repeats its connectors,
        // while the metrics cache supplies the outlines those pieces are drawn
        // with. Never scale a delimiter: a font publishes larger designs instead.
        if let Some(plan) = font_facts()
            .and_then(|facts| font_facts_glyph(key).map(|glyph| (facts, glyph)))
            .and_then(|(facts, glyph)| facts.variants().stretch(glyph, StretchAxis::Vertical, target_height / size))
        {
            if let Some(node) = self.draw_stretch_plan(&plan, &construction.variants, &construction.components, size)? {
                return Ok(self.center_delimiter(node, axis_height));
            }
        }

        // 2. Constructions a `MATH` table does not publish — the builtin bars are
        // declared rectangle geometry — fall back to the sizes they declare.
        let mut tallest: Option<&GlyphKey> = None;
        let mut tallest_extent = Fixed::ZERO;
        for variant in &construction.variants {
            let metrics = self.font_system.get_metrics(&variant.glyph)?;
            let extent = (metrics.height + metrics.depth) * size;
            if extent >= target_height {
                let glyph = self.layout_scaled_symbol(&variant.glyph, size)?;
                return Ok(self.center_delimiter(glyph, axis_height));
            }
            if extent > tallest_extent {
                tallest_extent = extent;
                tallest = Some(&variant.glyph);
            }
        }

        // 3. No declared design reaches the target; the tallest one still covers
        // best and the base glyph is the last resort.
        let glyph = self.layout_scaled_symbol(tallest.unwrap_or(key), size)?;
        Ok(self.center_delimiter(glyph, axis_height))
    }

    /// Draw a delimiter stretch plan with the outlines the metrics cache carries.
    ///
    /// Returns `Ok(None)` when the cache holds no outline for a piece the plan
    /// needs, so the caller can fall back to a declared size instead of drawing
    /// an invented design.
    fn draw_stretch_plan(
        &self,
        plan: &StretchPlan<'_>,
        variants: &[DelimiterVariant],
        components: &[DelimiterComponent],
        size: Fixed,
    ) -> MathResult<Option<LayoutNode>> {
        let find_variant =
            |glyph: u16| variants.iter().map(|variant| &variant.glyph).find(|key| key.glyph_id == Some(glyph)).cloned();

        match plan {
            StretchPlan::Variant(variant) => {
                let Some(key) = find_variant(variant.glyph.get())
                else {
                    return Ok(None);
                };
                if self.font_system.get_glyph_path(&key).is_err() {
                    return Ok(None);
                }
                Ok(Some(self.layout_scaled_symbol(&key, size)?))
            }
            StretchPlan::Assembly(plan) => {
                // Parts stack bottom-up: each piece sits on the previous one with
                // the font's `MinConnectorOverlap` at the junction, so the stack
                // spans `extent` rather than the sum of every part.
                let mut parts = Vec::with_capacity(plan.instances.len() * 2);
                for part in &plan.instances {
                    let key =
                        components.iter().map(|component| &component.glyph).find(|key| key.glyph_id == Some(part.glyph.get()));
                    let Some(key) = key
                    else {
                        return Ok(None);
                    };
                    if self.font_system.get_glyph_path(key).is_err() {
                        return Ok(None);
                    }
                    if !parts.is_empty() {
                        parts.push(LayoutNode::Kern(Fixed::ZERO - plan.overlap * size));
                    }
                    parts.push(self.layout_scaled_symbol(key, size)?);
                }
                Ok(Some(self.pack_vbox(parts, Alignment::Center)))
            }
        }
    }

    fn center_delimiter(&self, node: LayoutNode, axis_height: Fixed) -> LayoutNode {
        let mut wrapper = self.pack_hbox(vec![node], None);
        if let LayoutNode::HBox(ref mut h) = wrapper {
            let delim_h = h.height;
            let delim_d = h.depth;
            h.shift = (delim_h - delim_d) / Fixed::from_f64(2.0) - axis_height;
        }
        wrapper
    }

    fn layout_accent(&self, accent: &GlyphKey, base: &SemanticNode, state: MathStyleState) -> MathResult<LayoutNode> {
        let base_layout = self.layout_node_state(base, state)?;
        let style = state.style;
        let style_scale = self.get_style_scale(style);
        let size = self.base_size * style_scale;

        if accent.char == Some(OVERBAR_ACCENT) {
            return Ok(self.layout_overbar(base_layout, size));
        }

        let accent_layout = self.layout_symbol(accent, style)?;

        let total_width = base_layout.width().max(accent_layout.width());

        // TeX aligns the accent's ink centre with the base's top accent
        // attachment point published by the font's MATH table, so the accent
        // left edge sits at `attachment - accent_width / 2` measured from the
        // base's own left edge. Glyphs without a MATH entry fall back to plain
        // centring.
        let centered_accent = (total_width - accent_layout.width()) / Fixed::from_f64(2.0);
        let base_offset = (total_width - base_layout.width()) / Fixed::from_f64(2.0);
        let accent_layout = match self.top_accent_offset(&base_layout) {
            Some(attachment) => {
                let left = base_offset + attachment - accent_layout.width() / Fixed::from_f64(2.0);
                self.pack_hbox(vec![LayoutNode::Kern(left), accent_layout], Some(total_width))
            }
            None => self.pack_hbox(vec![LayoutNode::Kern(centered_accent), accent_layout], Some(total_width)),
        };

        // The embedded font draws its modifier marks for text use, so their ink
        // sits high above their own baseline (`ˆ` starts at 0.516 em). Stacking
        // the mark's baseline on the base's ink top would therefore leave it
        // floating most of an em above the letter. Replace the accent box's
        // extents with the ink it really paints, plus the gap underneath, so the
        // VBox below lands the ink bottom at that gap above the base's height.
        // TeX clears an accent glyph from the nucleus by the default rule
        // thickness, which the MATH table publishes as
        // `FractionRuleThickness`, while `\bar` keeps the overbar gap in
        // `layout_overbar`.
        let clearance = self.math_constant(MathConstant::FractionRuleThickness, size).unwrap_or(Fixed::ZERO);
        let flat_limit = self.math_constant(MathConstant::AccentBaseHeight, size);
        let flattened_limit = self.math_constant(MathConstant::FlattenedAccentBaseHeight, size);
        let gap = Self::accent_gap(base_layout.height(), clearance, flat_limit, flattened_limit);
        let accent_layout = fit_accent_extents(accent_layout, gap);

        let base_hbox = self.pack_hbox(vec![LayoutNode::Kern(base_offset), base_layout], Some(total_width));

        Ok(self.pack_vbox(vec![accent_layout, base_hbox], Alignment::Center))
    }

    /// Vertical gap between the base's ink top and the accent's ink bottom.
    ///
    /// `AccentBaseHeight` is the tallest base an accent may sit on flat, so a
    /// taller nucleus is raised until the accent clears it by `clearance`.
    /// `FlattenedAccentBaseHeight` is the height past which an accent must not
    /// grow the formula any further: above it the accent stays at that height
    /// and its ink overlaps the base's upper region instead of following the
    /// base up. A font that publishes only the flat limit caps the accent there.
    fn accent_gap(base_height: Fixed, clearance: Fixed, flat_limit: Option<Fixed>, flattened_limit: Option<Fixed>) -> Fixed {
        let flattened = flattened_limit.or(flat_limit);
        let rise = match flattened {
            Some(flattened) if base_height > flattened => flattened,
            _ => base_height,
        };
        rise + clearance - base_height
    }

    /// Lay out `\bar` / `\overline` as a rule over the base.
    ///
    /// The font's macron is a text modifier whose ink spans 0.36 em, so it can
    /// neither cover a normal nucleus nor read as an overbar. TeX paints an
    /// overbar as a rule spanning the nucleus, sized from the MATH table's
    /// overbar constants, which is what TeX's own `\bar` produces.
    fn layout_overbar(&self, base_layout: LayoutNode, size: Fixed) -> LayoutNode {
        let thickness = self.math_constant(MathConstant::OverbarRuleThickness, size).unwrap_or(Fixed::ZERO);
        let gap = self.math_constant(MathConstant::OverbarVerticalGap, size).unwrap_or(Fixed::ZERO);
        let width = base_layout.width();

        // A `Rule`'s baseline is its bottom edge, so the bar paints from the
        // accent baseline upwards. Declaring `gap` as the box depth reserves the
        // space between the bar and the base's ink top.
        let accent_layout = LayoutNode::HBox(Box::new(HBox {
            width,
            height: thickness,
            depth: gap,
            children: vec![LayoutNode::Rule { width, height: thickness, depth: Fixed::ZERO }],
            shift: Fixed::ZERO,
            glue_set: 0.0,
        }));

        self.pack_vbox(vec![accent_layout, base_layout], Alignment::Center)
    }

    /// Horizontal offset of a base's top accent attachment from its left edge.
    ///
    /// Walks left to right through horizontal material so a base made of
    /// several glyphs still reports the attachment of its leading glyph;
    /// vertical boxes and rules carry no attachment. The `MATH` table's
    /// `MathGlyphInfo` is the truth source; a glyph it does not cover keeps the
    /// attachment the metrics cache measured, which came from the same table.
    fn top_accent_offset(&self, node: &LayoutNode) -> Option<Fixed> {
        match node {
            LayoutNode::Glyph(glyph) => {
                let facts_attachment = glyph
                    .glyph_id
                    .and_then(|glyph_id| font_facts()?.top_accent_attachment(GlyphId::new(glyph_id)))
                    .map(|em| em * glyph.size);
                facts_attachment.filter(|attachment| *attachment > Fixed::ZERO).or_else(|| {
                    (glyph.metrics.top_accent_attachment > Fixed::ZERO).then_some(glyph.metrics.top_accent_attachment)
                })
            }
            LayoutNode::HBox(hbox) => {
                let mut offset = Fixed::ZERO;
                for child in &hbox.children {
                    if let Some(attachment) = self.top_accent_offset(child) {
                        return Some(offset + attachment);
                    }
                    offset = offset + child.width();
                }
                None
            }
            _ => None,
        }
    }
}

/// Replace an accent box's declared extents with the ink its glyph paints.
///
/// The box that carries an accent is a plain HBox, so its extents come from the
/// mark's advance metrics. Those metrics describe the whole em box, not where
/// the ink actually sits, and the embedded font draws its marks high above the
/// baseline. Rewriting the extents to `ink top` and `gap` below the ink bottom
/// makes the enclosing VBox land the mark's ink at that gap above the base
/// instead of floating a baseline's worth above it.
///
/// `gap` is the clearance the MATH constants call for, and it is negative for an
/// accent that has flattened against a tall base: then the VBox lets the mark's
/// ink overlap the base by exactly that much.
///
/// A glyph the font ships no outline for keeps its measured extents, so the
/// synthetic mark still stacks predictably.
fn fit_accent_extents(accent: LayoutNode, gap: Fixed) -> LayoutNode {
    let LayoutNode::HBox(mut hbox) = accent
    else {
        return accent;
    };
    let Some((ink_bottom, ink_top)) = hbox.children.iter().find_map(accent_ink_extents)
    else {
        return LayoutNode::HBox(hbox);
    };
    hbox.height = ink_top;
    // The ink starts above the accent's own baseline, so this depth is usually
    // negative. The VBox still sees the ink's height plus the gap as the accent's
    // line height, and the base follows it.
    hbox.depth = gap - ink_bottom;
    LayoutNode::HBox(hbox)
}

/// `(ink bottom, ink top)` of the first glyph inside `node`, in pixels above the
/// baseline, or `None` when that glyph has no outline to measure.
fn accent_ink_extents(node: &LayoutNode) -> Option<(Fixed, Fixed)> {
    match node {
        LayoutNode::Glyph(glyph) => {
            let path = glyph.path_data.as_deref()?;
            let (bottom, top) = super::collect::path_vertical_extent(path);
            let size = glyph.size.to_f64();
            Some((Fixed::from_f64(bottom * size), Fixed::from_f64(top * size)))
        }
        LayoutNode::HBox(hbox) => hbox.children.iter().find_map(accent_ink_extents),
        _ => None,
    }
}

/// 递归遍历 [`LayoutNode`] 树，收集每个有 `font_id` + `glyph_id` + `path_data`
/// 的 Glyph，产出 `(font_id, glyph_id, path_data)` 三元组，填充
/// [`FontMetricsSnapshot::glyph_paths`]。
fn collect_glyph_paths(node: &LayoutNode, out: &mut Vec<(String, u16, String)>) {
    match node {
        LayoutNode::HBox(hbox) => {
            for child in &hbox.children {
                collect_glyph_paths(child, out);
            }
        }
        LayoutNode::VBox(vbox) => {
            for child in &vbox.children {
                collect_glyph_paths(child, out);
            }
        }
        LayoutNode::Glyph(g) => {
            if let (Some(font_id), Some(glyph_id), Some(path_data)) = (&g.font_id, g.glyph_id, &g.path_data) {
                out.push((font_id.clone(), glyph_id, path_data.clone()));
            }
        }
        _ => {}
    }
}

/// 递归把 [`LayoutNode`] 树降成 [`PaintNode`] 树。
///
/// * HBox / VBox → [`PaintNode::Group`]，递归子节点。
/// * Glyph → 优先 [`PaintNode::GlyphRun`]（font_id + glyph_ids + positions），
///   缺 glyph_id 时回退 [`PaintNode::VectorPath`]（SVG outline），再兜底空
///   Group。
/// * Rule → [`PaintNode::Rect`]。
/// * Path → [`PaintNode::VectorPath`]。
/// * Kern / Glue / Penalty → 空 Group（不 paint，advance 进 metrics）。
fn lower_layout_to_paint(node: &LayoutNode, base_size: Fixed) -> PaintNode {
    match node {
        LayoutNode::HBox(hbox) => {
            let children: Vec<_> = hbox.children.iter().map(|c| lower_layout_to_paint(c, base_size)).collect();
            PaintNode::Group(children)
        }
        LayoutNode::VBox(vbox) => {
            let children: Vec<_> = vbox.children.iter().map(|c| lower_layout_to_paint(c, base_size)).collect();
            PaintNode::Group(children)
        }
        LayoutNode::Glyph(glyph) => {
            // 有 glyph_id 就走 GlyphRun；这是 PaintNode 里字体友好的表示。
            if glyph.glyph_id.is_some() {
                return PaintNode::GlyphRun {
                    font_id: glyph.font_id.clone().unwrap_or_default(),
                    glyph_ids: vec![glyph.glyph_id.unwrap_or(0)],
                    positions: vec![(Fixed::ZERO, Fixed::ZERO)],
                    size: glyph.size,
                    path_data: vec![glyph.path_data.clone()],
                };
            }
            // 没 glyph_id 但有 outline path：走 VectorPath。
            if let Some(ref path) = glyph.path_data {
                let bbox = BBox {
                    x: Fixed::ZERO,
                    y: Fixed::ZERO - glyph.metrics.height,
                    width: glyph.metrics.width,
                    height: glyph.metrics.height + glyph.metrics.depth,
                };
                return PaintNode::VectorPath { d: path.clone(), bbox };
            }
            // 兜底：空 Group。renderer 应该能接受，但这表示 layout
            // 没为这个 glyph 准备好任何可见内容。
            let _ = base_size; // 当前 Glyph.size 已足够，留签名给将来 fallback
            PaintNode::Group(Vec::new())
        }
        LayoutNode::Rule { width, height, depth } => PaintNode::Rect {
            x: Fixed::ZERO,
            y: Fixed::ZERO - *height,
            width: *width,
            height: *height + *depth,
            stroke_width: Fixed::ZERO,
        },
        LayoutNode::Path(path) => PaintNode::VectorPath {
            d: path.d.clone(),
            bbox: BBox { x: Fixed::ZERO, y: Fixed::ZERO - path.height, width: path.width, height: path.height + path.depth },
        },
        LayoutNode::Kern(_) | LayoutNode::Glue(_) | LayoutNode::Penalty { .. } => {
            // 只推进位置，不 paint。
            PaintNode::Group(Vec::new())
        }
    }
}

/// 从 [`LayoutNode`] 树根节点收集 [`FormulaMetrics`]。
///
/// 递归累加整棵树的 extents，避免依赖根节点必须是 HBox / VBox。
/// width 是所有节点 advance 的总和（或 root HBox 的 width），height 是
/// 所有子树的上沿到 baseline 的最大值，depth 是 baseline 下沿的最大值
/// （正数表示 descended below baseline）。
///
/// `font_system` 参数预留了未来查 `AxisHeight` / 字体快照的入口；当前
/// 版本 `axis` 仍返回 ZERO，等 [`FontMetricsSnapshot`] 真正填充后再查。
/// `ink_bounds` 用 width / height / depth 推导一个以 baseline 为 y=0
/// 的矩形。
fn extract_metrics(root: &LayoutNode, font_system: &FontMetricsSystem) -> FormulaMetrics {
    let (width, height, depth) = compute_extents(root);

    // axis 暂时留 ZERO — FontMetricsSnapshot 还是空的。等 snapshot 接入后：
    // let family = "default";
    // let axis = font_system.get_math_constant(family, MathConstant::AxisHeight)
    //     .unwrap_or(Fixed::from_f64(0.25)) * /* base size */;
    let _ = font_system;
    let axis = Fixed::ZERO;

    FormulaMetrics {
        width,
        height,
        depth,
        axis,
        ink_bounds: BBox { x: Fixed::ZERO, y: Fixed::ZERO - height, width, height: height + depth },
    }
}

/// 递归计算一个 [`LayoutNode`] 子树的 (width, height, depth)。
///
/// * width: 水平 advance 总量（HBox 累加所有 children，VBox 取最宽的 child）
/// * height: 子树上沿到 baseline 的最大距离（正数）
/// * depth: 子树下沿到 baseline 的最大距离（正数）
fn compute_extents(node: &LayoutNode) -> (Fixed, Fixed, Fixed) {
    match node {
        LayoutNode::HBox(h) => {
            let mut width = Fixed::ZERO;
            let mut max_height = Fixed::ZERO;
            let mut max_depth = Fixed::ZERO;
            for child in &h.children {
                let (cw, ch, cd) = compute_extents(child);
                width = width + cw;
                max_height = max_height.max(ch);
                max_depth = max_depth.max(cd);
            }
            // HBox 自带 shift：向上（负 shift）减小 height，增大 depth；
            // 向下（正 shift）相反。与 HBox::height / HBox::depth 的语义
            // 完全对齐，所以直接用打包好的值加上 shift 修正即可。
            width = width.max(h.width); // 打包值已经考虑了 glue_set 拉伸
            let height = h.height - h.shift;
            let depth = h.depth + h.shift;
            (width, height, depth.max(max_depth))
        }
        LayoutNode::VBox(v) => {
            let mut max_width = Fixed::ZERO;
            let mut total_height = Fixed::ZERO;
            let mut total_depth = Fixed::ZERO;
            for child in &v.children {
                let (cw, ch, cd) = compute_extents(child);
                max_width = max_width.max(cw);
                total_height = total_height + ch;
                total_depth = total_depth + cd;
            }
            // VBox 也可以直接用打包值，shift 只水平移动不影响 extents
            max_width = max_width.max(v.width);
            (max_width, v.height, v.depth)
        }
        LayoutNode::Glyph(g) => (g.metrics.width, g.metrics.height, g.metrics.depth),
        LayoutNode::Glue(g) => (g.width, Fixed::ZERO, Fixed::ZERO),
        LayoutNode::Kern(k) => (*k, Fixed::ZERO, Fixed::ZERO),
        LayoutNode::Rule { width, height, depth } => (*width, *height, *depth),
        LayoutNode::Path(p) => (p.width, p.height, p.depth),
        LayoutNode::Penalty { width, .. } => (*width, Fixed::ZERO, Fixed::ZERO),
    }
}
