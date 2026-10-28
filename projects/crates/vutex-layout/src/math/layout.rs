//! Layout tree (boxes, glue, kerns, glyphs) and the layout engine that lowers a
//! `SemanticNode` tree into it.

use serde::{Deserialize, Serialize};
use vutex_text::math::FontMetricsSystem;
use vutex_types::math::{
    Alignment, DelimiterComponent, Fixed, FontStyle, GlyphKey, GlyphMetrics, LineStyle, MathConstant, MathResult, MathStyle,
    SemanticNode, SpacingRule, SymbolRole,
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
            LayoutNode::Glyph(g) => {
                let mut w = g.metrics.width;
                // Italic / math-style glyphs carry an `italic_correction` from
                // the font that TeX appends to the advance width so slanted
                // characters don't collide with what follows on the right.
                if matches!(g.font_style, FontStyle::Italic | FontStyle::Math) {
                    w = w + g.metrics.italic_correction;
                }
                w
            }
            LayoutNode::Glue(g) => g.width,
            LayoutNode::Kern(k) => *k,
            LayoutNode::Rule { width, .. } => *width,
            LayoutNode::Path(p) => p.width,
            LayoutNode::Penalty { width, .. } => *width,
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

/// Lower bound on how far the radical sign may be compressed vertically.
const MIN_RADICAL_STRETCH: f64 = 0.4;
/// Upper bound on how far the radical sign may be stretched vertically.
const MAX_RADICAL_STRETCH: f64 = 3.0;

/// The accent character `\bar` / `\overline` is built from.
///
/// Unlike `\hat`, `\tilde`, `\dot`, `\ddot` and `\vec`, an overbar is a rule
/// spanning the nucleus rather than a letterform, so it never paints a glyph.
const OVERBAR_ACCENT: char = '¯';

/// 1 mu = 1/18 em. TeX thick/medium/thin muskip in em units.
const THICK_MUSIP_EM: f64 = 5.0 / 18.0;
const MEDIUM_MUSIP_EM: f64 = 4.0 / 18.0;
const THIN_MUSIP_EM: f64 = 3.0 / 18.0;

/// Lowers [`SemanticNode`] trees into [`LayoutNode`] boxes.
#[derive(Debug)]
pub struct LayoutEngine {
    font_system: FontMetricsSystem,
    base_size: Fixed,
    /// Sticky override from the most recent `\limits` / `\nolimits` that has
    /// not yet been consumed by a `LargeOperator`.
    limits_override: std::cell::Cell<Option<bool>>,
}

impl LayoutEngine {
    /// Create an engine with a base em size of `1.0`.
    pub fn new(font_system: FontMetricsSystem) -> Self {
        Self { font_system, base_size: Fixed::ONE, limits_override: std::cell::Cell::new(None) }
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
            | SemanticNode::Accent { .. }
            | SemanticNode::HorizontalBox { .. }
            | SemanticNode::VerticalBox { .. }
            | SemanticNode::Paragraph { .. } => SymbolRole::Inner,
            SemanticNode::Sequence(_)
            | SemanticNode::Text { .. }
            | SemanticNode::SetMathStyle(_)
            | SemanticNode::LimitsSpec(_) => SymbolRole::Ordinary,
        }
    }

    /// TeX math spacing between two adjacent atoms, simplified to the common cases.
    ///
    /// Returns the glue to insert between `left` and `right` at the current em
    /// `size` (in pixels).  `None` means no glue — but binary atoms on the wrong
    /// side are treated as [`SymbolRole::Ordinary`] (caller must rerun or skip).
    fn spacing_between(left: SymbolRole, right: SymbolRole, size: Fixed) -> Option<Glue> {
        use SymbolRole::*;
        // Opening on the left, Closing on the right, or Punctuation on the right
        // never take spacing.
        if matches!(left, Opening) || matches!(right, Closing | Punctuation) {
            return None;
        }

        match (left, right) {
            // ——— Relation: thick on both sides ———
            (_, Relation) | (Relation, _) => {
                Some(Glue { width: Fixed::from_f64(THICK_MUSIP_EM) * size, stretch: Fixed::ZERO, shrink: Fixed::ZERO })
            }

            // ——— LargeOperator: medium on both sides, but NOT after Opening ———
            (LargeOperator, _) => {
                Some(Glue { width: Fixed::from_f64(MEDIUM_MUSIP_EM) * size, stretch: Fixed::ZERO, shrink: Fixed::ZERO })
            }
            (_, LargeOperator) => {
                if matches!(left, Opening) {
                    None
                }
                else {
                    Some(Glue { width: Fixed::from_f64(MEDIUM_MUSIP_EM) * size, stretch: Fixed::ZERO, shrink: Fixed::ZERO })
                }
            }

            // ——— Operator (text math function like sin, cos): medium both sides ———
            (_, Operator) | (Operator, _) => {
                Some(Glue { width: Fixed::from_f64(MEDIUM_MUSIP_EM) * size, stretch: Fixed::ZERO, shrink: Fixed::ZERO })
            }

            // ——— Inner (fraction, radical, delimited group): thick both sides ———
            (_, Inner) | (Inner, _) => {
                Some(Glue { width: Fixed::from_f64(THICK_MUSIP_EM) * size, stretch: Fixed::ZERO, shrink: Fixed::ZERO })
            }

            // ——— Binary: thick glue when the neighbour can take one ———
            // Skip glue on binary → binary / open / close / rel / punct transitions
            // (TeX demotes the binary there; we keep it simple and just omit glue).
            (Binary, _) => {
                if matches!(right, Binary | Opening | Relation | Closing | Punctuation) {
                    None
                }
                else {
                    Some(Glue { width: Fixed::from_f64(THICK_MUSIP_EM) * size, stretch: Fixed::ZERO, shrink: Fixed::ZERO })
                }
            }
            (_, Binary) => {
                if matches!(left, Binary | Opening | Relation | Closing | Punctuation) {
                    None
                }
                else {
                    Some(Glue { width: Fixed::from_f64(THICK_MUSIP_EM) * size, stretch: Fixed::ZERO, shrink: Fixed::ZERO })
                }
            }

            // ——— Punctuation: thin after, nothing before ———
            (Punctuation, _) => {
                Some(Glue { width: Fixed::from_f64(THIN_MUSIP_EM) * size, stretch: Fixed::ZERO, shrink: Fixed::ZERO })
            }

            // Ordinary / Text adjacency: no glue
            _ => None,
        }
    }

    /// Whether a large operator stacks its scripts by default in display style.
    ///
    /// TeX's `op` class (`\sum`, `\prod`, `\coprod`) puts limits above/below,
    /// while the `intop` class (`\int`, `\iint`, `\oint`) never does unless
    /// `\limits` asks for it.
    fn default_limits(base: &SemanticNode) -> bool {
        match base {
            SemanticNode::Symbol { glyph_key, .. } => !matches!(glyph_key.char, Some('∫' | '∬' | '∮')),
            _ => true,
        }
    }

    /// Lay out a semantic node in the given math style.
    pub fn layout_node<'a>(&'a self, node: &'a SemanticNode, style: MathStyle) -> MathResult<LayoutNode> {
        {
            match node {
                SemanticNode::Symbol { glyph_key, .. } => self.layout_symbol(glyph_key, style),
                SemanticNode::Sequence(nodes) => self.layout_sequence(nodes, style),
                SemanticNode::HorizontalBox { content, spacing } => {
                    let mut children = Vec::new();
                    let n = content.len();
                    for (i, node) in content.iter().enumerate() {
                        children.push(self.layout_node(node, style)?);
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
                        children.push(self.layout_node(node, style)?);
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
                SemanticNode::Fraction { num, den, line } => self.layout_fraction(num, den, *line, style),
                SemanticNode::Radical { degree, radicand } => self.layout_radical(degree.as_deref(), radicand, style),
                SemanticNode::VerticalBox { content, alignment } => {
                    let mut children = Vec::new();
                    for node in content {
                        children.push(self.layout_node(node, style)?);
                    }
                    Ok(self.pack_vbox(children, *alignment))
                }
                SemanticNode::Text { text, font_style } => {
                    let mut children = Vec::new();
                    for c in text.chars() {
                        let key = GlyphKey::from_char(c, None, *font_style);
                        children.push(self.layout_symbol(&key, style)?);
                    }
                    Ok(self.pack_hbox(children, None))
                }
                SemanticNode::Subscript { base, sub } => self.layout_subsup(base, Some(sub), None, style),
                SemanticNode::Superscript { base, sup } => self.layout_subsup(base, None, Some(sup), style),
                SemanticNode::SubSuperscript { base, sub, sup } => self.layout_subsup(base, Some(sub), Some(sup), style),
                SemanticNode::Matrix { rows, row_spacing, col_spacing, column_alignment } => {
                    self.layout_matrix(rows, *row_spacing, *col_spacing, column_alignment, style)
                }
                SemanticNode::Delimited { left, right, content } => {
                    self.layout_delimited(left.as_ref(), right.as_ref(), content, style)
                }
                SemanticNode::Accent { accent, base } => self.layout_accent(accent, base, style),
                // Imperative nodes: no boxes; side-effects are handled by
                // `layout_sequence` and `layout_subsup` when they iterate.
                SemanticNode::SetMathStyle(_) | SemanticNode::LimitsSpec(_) => Ok(LayoutNode::Kern(Fixed::ZERO)),
            }
        }
    }

    /// Lay out a Sequence with imperative style tracking.
    ///
    /// `SetMathStyle` updates the active style for subsequent atoms,
    /// `LimitsSpec` sets a pending flag that `layout_subsup` pops when the
    /// next `LargeOperator` base is seen. Neither produces boxes itself.
    fn layout_sequence(&self, nodes: &[SemanticNode], mut style: MathStyle) -> MathResult<LayoutNode> {
        let mut children = Vec::new();
        let mut prev_role: Option<SymbolRole> = None;

        for node in nodes {
            // --- imperative nodes: update state, produce no boxes ---
            match node {
                SemanticNode::SetMathStyle(new_style) => {
                    style = *new_style;
                    continue;
                }
                SemanticNode::LimitsSpec(flag) => {
                    self.limits_override.set(Some(*flag));
                    continue;
                }
                // A bare operator carries no scripts to stack, so drop the flag
                // instead of letting it bleed onto a later operator.
                SemanticNode::Symbol { role: SymbolRole::LargeOperator, .. } => {
                    self.limits_override.set(None);
                }
                _ => {}
            }

            // --- spacing glue between the previous visible node and this one ---
            let style_scale = self.get_style_scale(style);
            let size = self.base_size * style_scale;
            if let Some(left_role) = prev_role {
                let right_role = Self::role_of(node);
                if let Some(glue) = Self::spacing_between(left_role, right_role, size) {
                    children.push(LayoutNode::Glue(glue));
                }
            }

            children.push(self.layout_node(node, style)?);
            prev_role = Some(Self::role_of(node));
        }
        Ok(self.pack_hbox(children, None))
    }

    fn layout_symbol(&self, key: &GlyphKey, style: MathStyle) -> MathResult<LayoutNode> {
        let style_scale = self.get_style_scale(style);
        let size = self.base_size * style_scale;
        self.layout_scaled_symbol(key, size)
    }

    fn layout_scaled_symbol(&self, key: &GlyphKey, size: Fixed) -> MathResult<LayoutNode> {
        let metrics = self.font_system.get_metrics(key)?;
        let path_data = self.font_system.get_glyph_path(key).ok();

        Ok(LayoutNode::Glyph(Glyph {
            char: key.char,
            font_family: key.font_family.clone().unwrap_or_else(|| "default".to_string()),
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
        }))
    }

    fn get_style_scale(&self, style: MathStyle) -> Fixed {
        match style {
            MathStyle::Display | MathStyle::Text => Fixed::ONE,
            MathStyle::Script => Fixed::from_f64(0.7),
            MathStyle::ScriptScript => Fixed::from_f64(0.5),
        }
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
        style: MathStyle,
    ) -> MathResult<LayoutNode> {
        // TeX demotes fraction arguments exactly one style: display -> text,
        // text -> script, and both script styles -> scriptscript.  Keeping
        // script arguments in script style would let them outgrow the parent.
        let next_style = match style {
            MathStyle::Display => MathStyle::Text,
            MathStyle::Text => MathStyle::Script,
            MathStyle::Script | MathStyle::ScriptScript => MathStyle::ScriptScript,
        };

        let num_layout = self.layout_node(numerator, next_style)?;
        let den_layout = self.layout_node(denominator, next_style)?;

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
        let actual_den_shift = den_shift.max(axis_height - rule_thickness / 2.0 + den_gap + den_height);
        v_children.push(LayoutNode::Kern(actual_den_shift - axis_height - rule_thickness / 2.0 - den_height));

        let den_hbox = self
            .pack_hbox(vec![LayoutNode::Kern((width - den_layout.width()) / Fixed::from_f64(2.0)), den_layout], Some(width));
        v_children.push(den_hbox);

        let mut vbox_node = self.pack_vbox(v_children, Alignment::Center);
        if let LayoutNode::VBox(ref mut v) = vbox_node {
            v.shift = Fixed::ZERO;
        }

        // Align the baseline of the VBox with the axis height by wrapping in HBox and shifting
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
        style: MathStyle,
    ) -> MathResult<LayoutNode> {
        let radicand_layout = self.layout_node(radicand, style)?;

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

        // Height of the radical bar above the radicand baseline. The sign is
        // raised so its own top bar lands exactly on this line and the two
        // strokes read as one continuous rule.
        let bar_top = radicand_layout.height() + vertical_gap + rule_thickness;
        // The sign reaches this far below the baseline so it hugs the radicand.
        let sign_bottom = radicand_layout.depth() + vertical_gap;

        // Resolve the real radical glyph from the font system; fall back to a
        // layout_scaled_symbol which produces a Glyph node (works for both SVG
        // and PDF backends — the Glyph has path_data when available, else text).
        let radical_key = GlyphKey::from_char('√', Some("default".to_string()), FontStyle::Normal);
        let mut radical_symbol = self.layout_scaled_symbol(&radical_key, size)?;

        // The embedded radical is designed for one em of radicand. Stretch it
        // along y only so it spans the requested bar height and bottom extent
        // without thickening its strokes sideways. The reported metrics then
        // follow the stretched outline, so its descender stays inside the box.
        if let LayoutNode::Glyph(glyph) = &mut radical_symbol {
            if let Some(path) = glyph.path_data.clone() {
                let height = glyph.metrics.height.to_f64();
                let natural = height + glyph.metrics.depth.to_f64();
                let mut stretched = path;
                if natural > f64::EPSILON {
                    let stretch = ((bar_top + sign_bottom).to_f64() / natural).clamp(MIN_RADICAL_STRETCH, MAX_RADICAL_STRETCH);
                    if (stretch - 1.0).abs() > 0.01 {
                        stretched = super::collect::scale_svg_path_d(&stretched, 1.0, stretch);
                    }
                }
                let (outline_bottom, outline_top) = super::collect::path_vertical_extent(&stretched);
                glyph.metrics.height = Fixed::from_f64(outline_top * size.to_f64());
                glyph.metrics.depth = Fixed::from_f64(-outline_bottom.min(0.0) * size.to_f64());
                glyph.path_data = Some(stretched);
            }
        }

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

    fn layout_limits(
        &self,
        base: &SemanticNode,
        sub: Option<&SemanticNode>,
        sup: Option<&SemanticNode>,
        style: MathStyle,
    ) -> MathResult<LayoutNode> {
        let next_style = MathStyle::Script;

        let family = "default";
        let style_scale = self.get_style_scale(style);
        let size = self.base_size * style_scale;

        let base_layout = self.layout_node(base, style)?;
        let axis_height =
            self.font_system.get_math_constant(family, MathConstant::AxisHeight).unwrap_or(Fixed::from_f64(0.25)) * size;
        let base_shift = axis_height - (base_layout.height() - base_layout.depth()) / Fixed::from_f64(2.0);

        let mut wrapper = self.pack_hbox(vec![base_layout], None);
        if let LayoutNode::HBox(ref mut h) = wrapper {
            h.shift = Fixed::ZERO - base_shift; // Negative shift moves UP
        }
        let base_layout = wrapper;

        // We need the widths of sup and sub to calculate the total width
        let mut sup_layout = None;
        if let Some(s) = sup {
            sup_layout = Some(self.layout_node(s, next_style)?);
        }
        let mut sub_layout = None;
        if let Some(s) = sub {
            sub_layout = Some(self.layout_node(s, next_style)?);
        }

        let total_width = base_layout
            .width()
            .max(sup_layout.as_ref().map(|n| n.width()).unwrap_or(Fixed::ZERO))
            .max(sub_layout.as_ref().map(|n| n.width()).unwrap_or(Fixed::ZERO));

        let mut v_children = Vec::new();

        if let Some(sup_l) = sup_layout {
            let gap =
                self.font_system.get_math_constant(family, MathConstant::UpperLimitGapMin).unwrap_or(Fixed::from_f64(0.1))
                    * size;
            let rise = self
                .font_system
                .get_math_constant(family, MathConstant::UpperLimitBaselineRiseMin)
                .unwrap_or(Fixed::from_f64(0.3))
                * size;
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

        let mut sub_height = Fixed::ZERO;
        let mut actual_drop = Fixed::ZERO;

        if let Some(sub_l) = sub_layout {
            let gap =
                self.font_system.get_math_constant(family, MathConstant::LowerLimitGapMin).unwrap_or(Fixed::from_f64(0.1))
                    * size;
            let drop = self
                .font_system
                .get_math_constant(family, MathConstant::LowerLimitBaselineDropMin)
                .unwrap_or(Fixed::from_f64(0.6))
                * size;
            actual_drop = gap.max(drop - base_depth);
            sub_height = sub_l.height();

            v_children.push(LayoutNode::Kern(actual_drop));
            let sub_hbox = self.pack_hbox(
                vec![LayoutNode::Kern((total_width - sub_l.width()) / Fixed::from_f64(2.0)), sub_l],
                Some(total_width),
            );
            v_children.push(sub_hbox);
        }

        let vbox_node = self.pack_vbox(v_children, Alignment::Center);

        if sub_height != Fixed::ZERO {
            let shift = base_depth + actual_drop + sub_height;

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

    fn layout_subsup(
        &self,
        base: &SemanticNode,
        sub: Option<&SemanticNode>,
        sup: Option<&SemanticNode>,
        style: MathStyle,
    ) -> MathResult<LayoutNode> {
        let is_large_op = matches!(base, SemanticNode::Symbol { role: SymbolRole::LargeOperator, .. });

        // Consume any pending limits override from `\limits` / `\nolimits`.
        // It applies to this LargeOperator only; for non-operator bases we
        // leave the override in place so it still hits a later operator.
        let use_limits = if is_large_op {
            match self.limits_override.take() {
                Some(flag) => flag,
                None => style == MathStyle::Display && Self::default_limits(base),
            }
        }
        else {
            style == MathStyle::Display
        };

        if is_large_op && use_limits {
            return self.layout_limits(base, sub, sup, style);
        }

        let base_layout = self.layout_node(base, style)?;

        let next_style = match style {
            MathStyle::Display | MathStyle::Text => MathStyle::Script,
            _ => MathStyle::ScriptScript,
        };

        let family = "default";
        let style_scale = self.get_style_scale(style);
        let size = self.base_size * style_scale;

        let mut script_vbox_children = Vec::new();
        let mut calculated_sub_shift = Fixed::ZERO;

        if let Some(sup_node) = sup {
            let sup_layout = self.layout_node(sup_node, next_style)?;
            let sup_shift =
                self.font_system.get_math_constant(family, MathConstant::SuperscriptShiftUp).unwrap_or(Fixed::from_f64(0.4))
                    * size;
            let sup_bottom_min =
                self.font_system.get_math_constant(family, MathConstant::SuperscriptBottomMin).unwrap_or(Fixed::from_f64(0.1))
                    * size;

            let calculated_sup_shift = sup_shift.max(base_layout.height() + sup_bottom_min);

            script_vbox_children.push(sup_layout);
            // This kern will be between Sup and the baseline (or Sub)
            script_vbox_children.push(LayoutNode::Kern(calculated_sup_shift));
        }

        if let Some(sub_node) = sub {
            let sub_layout = self.layout_node(sub_node, next_style)?;
            let sub_shift =
                self.font_system.get_math_constant(family, MathConstant::SubscriptShiftDown).unwrap_or(Fixed::from_f64(0.2))
                    * size;
            let sub_top_max =
                self.font_system.get_math_constant(family, MathConstant::SubscriptTopMax).unwrap_or(Fixed::from_f64(0.4))
                    * size;

            calculated_sub_shift = sub_shift.max(sub_layout.height() - sub_top_max);

            // If we already have a superscript, we need to adjust the kern between them
            if script_vbox_children.len() == 2 {
                if let LayoutNode::Kern(sup_kern) = script_vbox_children.pop().unwrap() {
                    let subsup_gap = self
                        .font_system
                        .get_math_constant(family, MathConstant::SubSuperscriptGapMin)
                        .unwrap_or(Fixed::from_f64(0.1))
                        * size;

                    let sup_layout_depth = script_vbox_children.last().map(|n| n.depth()).unwrap_or(Fixed::ZERO);
                    let total_dist = (sup_kern + calculated_sub_shift).max(sup_layout_depth + subsup_gap + sub_layout.height());

                    // We'll push a kern that is the distance from Sup baseline to Sub baseline.
                    script_vbox_children.push(LayoutNode::Kern(total_dist));
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
                        h.shift = Fixed::ZERO;
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
        children.push(final_scripts);

        Ok(self.pack_hbox(children, None))
    }

    fn layout_matrix(
        &self,
        rows: &[Vec<SemanticNode>],
        row_spacing: Fixed,
        col_spacing: Fixed,
        column_alignment: &[Alignment],
        style: MathStyle,
    ) -> MathResult<LayoutNode> {
        if rows.is_empty() {
            return Ok(LayoutNode::Kern(Fixed::ZERO));
        }

        let num_rows = rows.len();
        let num_cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);

        if num_cols == 0 {
            return Ok(LayoutNode::Kern(Fixed::ZERO));
        }

        let style_scale = self.get_style_scale(style);
        let size = self.base_size * style_scale;

        let row_spacing = row_spacing * size;
        let col_spacing = col_spacing * size;

        // 1. Layout all cells and find column widths and row heights/depths
        let mut cell_layouts = vec![vec![None; num_cols]; num_rows];
        let mut col_widths = vec![Fixed::ZERO; num_cols];

        for r in 0..num_rows {
            for c in 0..rows[r].len() {
                let layout = self.layout_node(&rows[r][c], style)?;
                col_widths[c] = col_widths[c].max(layout.width());
                cell_layouts[r][c] = Some(layout);
            }
        }

        // 2. Pack cells into rows
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

        // Center the matrix around the math axis
        let family = "default";
        let axis_height =
            self.font_system.get_math_constant(family, MathConstant::AxisHeight).unwrap_or(Fixed::from_f64(0.25)) * size;

        let (vbox_height, vbox_depth) = match &matrix_vbox {
            LayoutNode::VBox(v) => (v.height, v.depth),
            _ => (Fixed::ZERO, Fixed::ZERO),
        };

        // VBox extends from -height to +depth relative to its baseline.
        // Center is at (depth - height) / 2.
        // We want (depth - height) / 2 + shift = -axis_height.
        // shift = (height - depth) / 2 - axis_height.
        let shift = (vbox_height - vbox_depth) / Fixed::from_f64(2.0) - axis_height;

        let mut wrapper = self.pack_hbox(vec![matrix_vbox], None);
        if let LayoutNode::HBox(ref mut h) = wrapper {
            // `LayoutNode::height` / `depth` already fold `shift` back into the
            // extents, exactly like `center_delimiter` relies on.  Writing the
            // shifted values into `height` / `depth` as well would apply the
            // shift twice and invert the box, so only the shift is stored.
            h.shift = shift;
        }
        Ok(wrapper)
    }

    fn layout_delimited(
        &self,
        left: Option<&GlyphKey>,
        right: Option<&GlyphKey>,
        content: &SemanticNode,
        style: MathStyle,
    ) -> MathResult<LayoutNode> {
        let content_layout = self.layout_node(content, style)?;

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

        // 1. Try variants first
        for variant in construction.variants {
            let metrics = self.font_system.get_metrics(&variant.glyph)?;
            let total_height = (metrics.height + metrics.depth) * size.to_f64();
            if total_height >= target_height {
                let glyph = self.layout_scaled_symbol(&variant.glyph, size)?;
                return Ok(self.center_delimiter(glyph, axis_height));
            }
        }

        // 2. If no variant is large enough, try assembly
        if !construction.components.is_empty() {
            return self.assemble_delimiter(&construction.components, target_height, size, axis_height);
        }

        // 3. Fallback: scale the original glyph
        let scale_factor = target_height / (Fixed::from_f64(0.7) * size); // Approx 0.7em for normal char
        let glyph = self.layout_scaled_symbol(key, size * scale_factor)?;
        Ok(self.center_delimiter(glyph, axis_height))
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

    fn assemble_delimiter(
        &self,
        components: &[DelimiterComponent],
        target_height: Fixed,
        size: Fixed,
        axis_height: Fixed,
    ) -> MathResult<LayoutNode> {
        let mut parts = Vec::new();

        // Calculate total height of non-extender parts
        let mut fixed_height = Fixed::ZERO;
        for comp in components {
            if !comp.is_extender {
                let metrics = self.font_system.get_metrics(&comp.glyph)?;
                fixed_height = fixed_height + (metrics.height + metrics.depth) * size.to_f64();
            }
        }

        // Simplified assembly: stack parts in order.
        // For vertical delimiters, the components table is usually ordered top-to-bottom.
        for comp in components {
            if comp.is_extender {
                let metrics = self.font_system.get_metrics(&comp.glyph)?;
                let h = (metrics.height + metrics.depth) * size.to_f64();
                let count =
                    if h > Fixed::ZERO { ((target_height - fixed_height).to_f64() / h.to_f64()).ceil() as usize } else { 1 };
                for _ in 0..count.max(1) {
                    parts.push(self.layout_scaled_symbol(&comp.glyph, size)?);
                }
            }
            else {
                parts.push(self.layout_scaled_symbol(&comp.glyph, size)?);
            }
        }

        let vbox = self.pack_vbox(parts, Alignment::Center);
        Ok(self.center_delimiter(vbox, axis_height))
    }

    fn layout_accent(&self, accent: &GlyphKey, base: &SemanticNode, style: MathStyle) -> MathResult<LayoutNode> {
        let base_layout = self.layout_node(base, style)?;
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
        let accent_layout = match Self::top_accent_offset(&base_layout) {
            Some(attachment) => {
                let left = base_offset + attachment - accent_layout.width() / Fixed::from_f64(2.0);
                self.pack_hbox(vec![LayoutNode::Kern(left), accent_layout], Some(total_width))
            }
            None => self.pack_hbox(vec![LayoutNode::Kern(centered_accent), accent_layout], Some(total_width)),
        };

        let base_hbox = self.pack_hbox(vec![LayoutNode::Kern(base_offset), base_layout], Some(total_width));

        // The embedded font draws its modifier marks for text use, so their ink
        // sits high above their own baseline (`ˆ` starts at 0.516 em). Stacking
        // the mark's baseline on the base's ink top would therefore leave it
        // floating most of an em above the letter. Replace the accent box's
        // extents with the ink it really paints, plus `clearance` underneath,
        // so the VBox below lands the ink bottom just above the base's height.
        let clearance = self
            .font_system
            .get_math_constant("default", MathConstant::OverbarVerticalGap)
            .unwrap_or(Fixed::from_f64(0.1))
            * size;
        let accent_layout = fit_accent_extents(accent_layout, clearance);

        Ok(self.pack_vbox(vec![accent_layout, base_hbox], Alignment::Center))
    }

    /// Lay out `\bar` / `\overline` as a rule over the base.
    ///
    /// The font's macron is a text modifier whose ink spans 0.36 em, so it can
    /// neither cover a normal nucleus nor read as an overbar. TeX paints an
    /// overbar as a rule spanning the nucleus, sized from the MATH table's
    /// overbar constants, which is what TeX's own `\bar` produces.
    fn layout_overbar(&self, base_layout: LayoutNode, size: Fixed) -> LayoutNode {
        let thickness = self
            .font_system
            .get_math_constant("default", MathConstant::OverbarRuleThickness)
            .unwrap_or(Fixed::from_f64(0.06))
            * size;
        let gap =
            self.font_system.get_math_constant("default", MathConstant::OverbarVerticalGap).unwrap_or(Fixed::from_f64(0.1))
                * size;
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
    /// vertical boxes and rules carry no attachment.
    fn top_accent_offset(node: &LayoutNode) -> Option<Fixed> {
        match node {
            LayoutNode::Glyph(glyph) => {
                let attachment = glyph.metrics.top_accent_attachment;
                (attachment > Fixed::ZERO).then_some(attachment)
            }
            LayoutNode::HBox(hbox) => {
                let mut offset = Fixed::ZERO;
                for child in &hbox.children {
                    if let Some(attachment) = Self::top_accent_offset(child) {
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
/// baseline. Rewriting the extents to `ink top` and `clearance` below the ink
/// bottom makes the enclosing VBox land the mark's ink just above the base
/// instead of floating a baseline's worth above it.
///
/// A glyph the font ships no outline for keeps its measured extents, so the
/// synthetic mark still stacks predictably.
fn fit_accent_extents(accent: LayoutNode, clearance: Fixed) -> LayoutNode {
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
    // negative. The VBox still sees a positive line height — the ink's height
    // plus the clearance — and the base follows it.
    hbox.depth = clearance - ink_bottom;
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
