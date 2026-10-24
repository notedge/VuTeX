//! Document layout pass: Notedown block tree to positioned page primitives.
//!
//! The pass walks [`DocumentGraph`] blocks in reading order, measures inline
//! content through `vutex-text` metrics, wraps lines greedily inside the
//! content width, and paginates at line granularity.

use std::collections::HashSet;

use notedown_ir::{Block, DocumentGraph, Inline, ListItem, NodeId, RelationEndpoint, TableRow};
use vutex_text::math::{FontMetricsSystem, builtin_font_metrics};
use vutex_types::math::{FontStyle, GlyphKey};

use crate::{
    diagram::Diagrams,
    hint::LayoutHint,
    math::{LayoutNode, collect_layout, layout_formula_with_size},
    plan::{Graphic, ImagePlaceholder, LayoutItem, LayoutPage, LayoutPlan, Rect, Rule, TextRun},
};

/// Text baseline height as a fraction of the font size.
const ASCENT: f64 = 0.8;
/// Height of a strikethrough rule above the text baseline, as a fraction of
/// the font size.
const STRIKE_OFFSET: f64 = 0.3;
/// Stroke width of a strikethrough rule, as a fraction of the font size.
const STRIKE_THICKNESS: f64 = 0.06;
/// Horizontal indent per list nesting level, in points.
const LIST_INDENT: f64 = 18.0;
/// Font family used for body text.
const SERIF: &str = "serif";
/// Font family used for code and preformatted text.
const MONOSPACE: &str = "monospace";
/// Fixed advance width of every monospace glyph, in em units.
const MONOSPACE_ADVANCE: f64 = 0.6;

/// Family and face style shared by consecutive inline text atoms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TextFace {
    /// Font family name carried into the text run.
    family: &'static str,
    /// Face style carried into the text run.
    style: FontStyle,
}

/// Body text face used by every unstyled run.
const BODY_FACE: TextFace = TextFace { family: SERIF, style: FontStyle::Normal };
/// Face used by inline code spans.
const CODE_FACE: TextFace = TextFace { family: MONOSPACE, style: FontStyle::Normal };

/// Measuring face for a run emitted with an explicit family name.
///
/// Only the two families layout emits reach this helper, so anything that is
/// not monospace measures as body text.
fn face_for_family(family: &str) -> TextFace {
    if family == MONOSPACE { CODE_FACE } else { BODY_FACE }
}

/// Map an inline style name onto the shared text face.
///
/// Only the emphasis names the document model recognises carry a visible
/// face; any other style string is laid out as body text instead of being
/// guessed at.
fn face_for_style(style: &str) -> TextFace {
    match style.to_ascii_lowercase().as_str() {
        "bold" | "strong" => TextFace { family: SERIF, style: FontStyle::Bold },
        "italic" | "em" => TextFace { family: SERIF, style: FontStyle::Italic },
        "mono" | "code" => TextFace { family: MONOSPACE, style: FontStyle::Monospace },
        "strike" => TextFace { family: SERIF, style: FontStyle::Strike },
        _ => BODY_FACE,
    }
}

/// Resolve the destination of a link target.
///
/// Only authored external URIs become destinations. Internal `Node`,
/// `Document`, `Asset`, and `Unresolved` targets stay without a destination,
/// because layout emits no matching anchor ids and a fabricated URL would point
/// nowhere.
fn link_href(target: &RelationEndpoint) -> Option<&str> {
    match target {
        RelationEndpoint::ExternalUri(uri) if !uri.is_empty() => Some(uri),
        _ => None,
    }
}

/// Resolve the hover text of a link target.
///
/// An authored empty title carries no tooltip, so it resolves to `None` instead
/// of an empty tooltip in the output.
fn link_title(title: Option<&String>) -> Option<&str> {
    title.map(String::as_str).filter(|value| !value.is_empty())
}

/// Whether a code fence language tag names a host-lowerable diagram producer.
///
/// Mermaid fences and TikZ pictures can carry a host-registered diagram scene.
/// Any other tag stays preformatted code even when a scene is registered for
/// its block.
fn is_diagram_language(language: Option<&str>) -> bool {
    language.is_some_and(|value| {
        let value = value.trim();
        value.eq_ignore_ascii_case("mermaid") || value.eq_ignore_ascii_case("tikz")
    })
}

/// Compute the layout plan for a Notedown document graph.
///
/// The document layout pass walks the Notedown block tree, measures inline
/// content through `vutex-text`, fills pages, and materialises positioned
/// items.  This module owns that pass; the crate root only re-exports it.
pub fn layout(graph: &DocumentGraph) -> LayoutPlan {
    layout_with_hint(graph, &LayoutHint::a4())
}

/// Compute the layout plan with explicit page geometry and body metrics.
pub fn layout_with_hint(graph: &DocumentGraph, hint: &LayoutHint) -> LayoutPlan {
    let diagrams = Diagrams::default();
    layout_with_diagrams(graph, hint, &diagrams)
}

/// Compute the layout plan, placing the diagram scenes the host registered for
/// `diagram` blocks.
///
/// A `diagram` block without a registered scene keeps its current behaviour and
/// is dropped, because layout never invents diagram geometry on its own.
pub fn layout_with_diagrams(graph: &DocumentGraph, hint: &LayoutHint, diagrams: &Diagrams) -> LayoutPlan {
    let mut builder = PageBuilder::new(*hint, diagrams);
    builder.render_blocks(graph, &root_ids(graph), 0.0);
    LayoutPlan { pages: builder.pages }
}

/// Block ids that no section or list item claims as a child, in graph order.
fn root_ids(graph: &DocumentGraph) -> Vec<NodeId> {
    let mut referenced = HashSet::new();
    for node in &graph.blocks {
        match &node.block {
            Block::Section { children, .. } => referenced.extend(children.iter().copied()),
            Block::List { items, .. } => {
                for item in items {
                    referenced.extend(item.children.iter().copied());
                }
            }
            _ => {}
        }
    }
    graph.blocks.iter().map(|node| node.id).filter(|id| !referenced.contains(id)).collect()
}

/// One indivisible atom inside a composed paragraph line.
#[derive(Debug, Clone)]
enum InlineAtom {
    /// Text break atom: a word, a single space, or one CJK glyph.
    Text {
        /// Atom text.
        text: String,
        /// Face shared with the surrounding styled span.
        face: TextFace,
        /// External destination shared with the surrounding link, if any.
        href: Option<String>,
        /// Hover text shared with the surrounding link, if any.
        title: Option<String>,
    },
    /// Laid-out inline formula, kept whole across line breaks.
    Math(MathAtom),
    /// Forced line ending produced by an authored hard break.
    Break,
}

/// Text buffered until the face, the link destination, or the link title
/// changes.
#[derive(Debug, Clone)]
struct PendingText {
    text: String,
    face: TextFace,
    href: Option<String>,
    title: Option<String>,
}

impl PendingText {
    fn body() -> Self {
        Self { text: String::new(), face: BODY_FACE, href: None, title: None }
    }

    fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Buffer `text` under `face`, `href`, and `title`, flushing first when any
    /// key changes.
    ///
    /// Splitting on a face, destination, or hover-text change is what turns a
    /// styled span or a link into its own [`LayoutItem`], so emphasis and link
    /// metadata survive into every renderer without swallowing the neighbouring
    /// text.
    fn push(&mut self, text: &str, face: TextFace, href: Option<&str>, title: Option<&str>, atoms: &mut Vec<InlineAtom>) {
        if text.is_empty() {
            return;
        }
        if !self.is_empty() && (self.face != face || self.href.as_deref() != href || self.title.as_deref() != title) {
            self.flush(atoms);
        }
        self.face = face;
        self.href = href.map(str::to_string);
        self.title = title.map(str::to_string);
        self.text.push_str(text);
    }

    /// Move the buffered text into break atoms.
    fn flush(&mut self, atoms: &mut Vec<InlineAtom>) {
        if self.is_empty() {
            self.text.clear();
            return;
        }
        atoms.extend(split_inline_atoms(&self.text, self.face, self.href.as_deref(), self.title.as_deref()));
        self.text.clear();
    }
}

/// How an inline image joins the atom flow of a block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImageMode {
    /// Flush the atoms at the cursor and reserve an image placeholder.
    Placeholder,
    /// Fold the alternative text into the text flow where no image box fits.
    AltText,
}

/// Atom buffer and pending text shared while walking inline content.
#[derive(Debug, Clone)]
struct AtomFlow {
    /// Composed break atoms in reading order.
    atoms: Vec<InlineAtom>,
    /// Text buffered until the face, destination, or hover text changes.
    pending: PendingText,
}

impl AtomFlow {
    /// Start a flow whose unstyled text uses `face`.
    fn new(face: TextFace) -> Self {
        Self { atoms: Vec::new(), pending: PendingText { text: String::new(), face, href: None, title: None } }
    }

    /// Close the buffered text into atoms and return the composed flow.
    fn finish(mut self) -> Vec<InlineAtom> {
        self.pending.flush(&mut self.atoms);
        self.atoms
    }
}

/// One table cell prepared for painting: wrapped lines and their column span.
#[derive(Debug, Clone)]
struct CellLines {
    /// Columns occupied by the cell.
    span: usize,
    /// Width available to the wrapped lines, in points.
    available: f64,
    /// Broken lines of the cell content.
    lines: Vec<Vec<InlineAtom>>,
}

/// Vertical extent of one composed line.
#[derive(Debug, Clone, Copy)]
struct LineMetrics {
    /// Extra space above the text line box taken by a tall formula.
    raise: f64,
    /// Extent from the raised line top to the lowest formula depth.
    height: f64,
}

/// A formula box carried alongside the paragraph flow.
#[derive(Debug, Clone)]
struct MathAtom {
    node: LayoutNode,
    width: f64,
    height: f64,
    depth: f64,
}

/// Page-filling state for one layout pass.
struct PageBuilder<'a> {
    hint: LayoutHint,
    metrics: FontMetricsSystem,
    pages: Vec<LayoutPage>,
    /// Scenes registered by the host for `diagram` blocks.
    diagrams: &'a Diagrams,
    /// Top edge of the next line box, in page coordinates.
    cursor: f64,
}

impl<'a> PageBuilder<'a> {
    fn new(hint: LayoutHint, diagrams: &'a Diagrams) -> Self {
        Self { hint, metrics: builtin_font_metrics(), pages: vec![blank_page(&hint)], diagrams, cursor: hint.margin.top }
    }

    fn push(&mut self, item: LayoutItem) {
        if let Some(page) = self.pages.last_mut() {
            page.items.push(item);
        }
    }

    fn page_has_items(&self) -> bool {
        self.pages.last().is_some_and(|page| !page.items.is_empty())
    }

    /// Skip inter-block spacing at the top of a page.
    fn pad(&mut self, amount: f64) {
        if self.page_has_items() {
            self.cursor += amount;
        }
    }

    /// Start a new page when `height` does not fit below the cursor.
    fn ensure_space(&mut self, height: f64) {
        if !self.page_has_items() {
            return;
        }
        if self.cursor + height > self.hint.content_bottom() {
            self.pages.push(blank_page(&self.hint));
            self.cursor = self.hint.margin.top;
        }
    }

    fn left(&self, indent: f64) -> f64 {
        self.hint.margin.left + indent
    }

    fn content_width(&self, indent: f64) -> f64 {
        (self.hint.content_width() - indent).max(24.0)
    }

    fn body_size(&self) -> f64 {
        self.hint.base_font_size
    }

    fn line_box(&self, size: f64) -> f64 {
        size * self.hint.line_height
    }

    fn wrap(&self, text: &str, size: f64, indent: f64) -> Vec<String> {
        wrap_text(&self.metrics, text, size, self.content_width(indent))
    }

    /// Paint one line at the cursor and advance past its line box.
    fn emit_line(&mut self, text: String, size: f64, family: &str, indent: f64) {
        let line_box = self.line_box(size);
        self.ensure_space(line_box);
        self.emit_baseline(text, size, family, indent);
        self.cursor += line_box;
    }

    /// Paint one line at the cursor without advancing, so a list marker can
    /// share the baseline of the first content line.
    fn emit_baseline(&mut self, text: String, size: f64, family: &str, indent: f64) {
        let x = self.left(indent);
        let y = self.cursor + ASCENT * size;
        let width = measure_text(&self.metrics, &text, size, face_for_family(family));
        self.push(LayoutItem::TextRun(TextRun {
            x,
            y,
            size,
            width,
            font_family: family.to_string(),
            style: FontStyle::Normal,
            text,
            href: None,
            title: None,
        }));
    }

    fn emit_wrapped(&mut self, text: &str, size: f64, family: &str, indent: f64) {
        for line in self.wrap(text, size, indent) {
            if line.is_empty() {
                continue;
            }
            self.emit_line(line, size, family, indent);
        }
    }

    fn emit_heading(&mut self, level: u8, title: &[Inline], indent: f64) {
        let base = self.body_size();
        let size = match level {
            1 => base * 1.8,
            2 => base * 1.45,
            3 => base * 1.25,
            _ => base * 1.1,
        };
        let line_box = self.line_box(size);
        let before = if level <= 2 { base } else { base * 0.8 };
        self.pad(before);
        // Keep the heading on the same page as its first body line.
        self.ensure_space(line_box * 2.0 + self.line_box(base));
        let atoms = self.compose_atoms(title, size, indent, BODY_FACE, ImageMode::AltText);
        self.emit_inline_lines(atoms, size, indent);
        self.pad(base * 0.4);
    }

    /// Compose a paragraph from text, styled spans, inline math, and image atoms.
    ///
    /// Text keeps the greedy break atoms of [`wrap_text`]; a style change
    /// splits the run so emphasis reaches the renderers. Inline math becomes a
    /// real formula box that flows on the text baseline and is never split.
    /// When the math engine rejects a formula, its raw source joins the text
    /// flow, matching the display-math fallback.
    fn emit_paragraph(&mut self, content: &[Inline], indent: f64) {
        let size = self.body_size();
        let atoms = self.compose_atoms(content, size, indent, BODY_FACE, ImageMode::Placeholder);
        self.emit_inline_lines(atoms, size, indent);
        self.pad(size * 0.6);
    }

    /// Compose inline content into break atoms under `face`.
    ///
    /// Paragraphs and every other inline-bearing block share this composition
    /// path, so emphasis keeps the same semantics wherever it appears.
    fn compose_atoms(
        &mut self,
        content: &[Inline],
        size: f64,
        indent: f64,
        face: TextFace,
        image_mode: ImageMode,
    ) -> Vec<InlineAtom> {
        let mut flow = AtomFlow::new(face);
        self.compose_inline(content, size, indent, &mut flow, face, None, None, image_mode);
        flow.finish()
    }

    /// Walk inline content into paragraph atoms under the enclosing face and
    /// link metadata.
    ///
    /// A link destination and hover text are inherited by nested inline
    /// content, so styled text inside an external link keeps its face, its
    /// destination, and its tooltip.
    fn compose_inline(
        &mut self,
        content: &[Inline],
        size: f64,
        indent: f64,
        flow: &mut AtomFlow,
        face: TextFace,
        href: Option<&str>,
        title: Option<&str>,
        image_mode: ImageMode,
    ) {
        for inline in content {
            match inline {
                Inline::Image { source, alt, .. } => {
                    let alt_text: String = alt.iter().map(Inline::plain_text).collect();
                    match image_mode {
                        ImageMode::Placeholder => {
                            flow.pending.flush(&mut flow.atoms);
                            self.emit_inline_lines(std::mem::take(&mut flow.atoms), size, indent);
                            self.emit_image_placeholder(&alt_text, source, indent);
                        }
                        ImageMode::AltText => flow.pending.push(&alt_text, face, href, title, &mut flow.atoms),
                    }
                }
                Inline::InlineMath { content, .. } => {
                    flow.pending.flush(&mut flow.atoms);
                    match layout_formula_with_size(content, false, size) {
                        Ok((node, formula)) => flow.atoms.push(InlineAtom::Math(MathAtom {
                            node,
                            width: formula.width,
                            height: formula.height,
                            depth: formula.depth,
                        })),
                        Err(_) => flow.atoms.extend(split_inline_atoms(content, face, href, title)),
                    }
                }
                Inline::Styled { style, children } => {
                    self.compose_inline(children, size, indent, flow, face_for_style(style), href, title, image_mode);
                }
                Inline::InlineCode { text } => flow.pending.push(text, CODE_FACE, href, title, &mut flow.atoms),
                // Attributes carry no printable face, but their wrapped content
                // keeps every authored style and destination.
                Inline::Attributed { children, .. } => {
                    self.compose_inline(children, size, indent, flow, face, href, title, image_mode);
                }
                Inline::Link { target, display, title: link_hover } => {
                    // A nested external link wins. A nested internal link keeps
                    // the enclosing destination so its text stays in the anchor.
                    let inner_href = link_href(target).or(href);
                    let inner_title = link_title(link_hover.as_ref()).or(title);
                    self.compose_inline(display, size, indent, flow, face, inner_href, inner_title, image_mode);
                }
                Inline::Note { display: Some(display), .. } | Inline::Citation { display: Some(display), .. } => {
                    self.compose_inline(display, size, indent, flow, face, href, title, image_mode);
                }
                other => flow.pending.push(&other.plain_text(), face, href, title, &mut flow.atoms),
            }
        }
    }

    /// Break inline atoms into lines and paint each composed line.
    fn emit_inline_lines(&mut self, atoms: Vec<InlineAtom>, size: f64, indent: f64) {
        let max_width = self.content_width(indent);
        for line in break_inline_atoms(&self.metrics, atoms, size, max_width) {
            self.emit_inline_line(&line, size, indent);
        }
    }

    /// Paint one composed line, sizing its box by the tallest formula it holds.
    fn emit_inline_line(&mut self, line: &[InlineAtom], size: f64, indent: f64) {
        let line_metrics = self.inline_line_metrics(line, size);
        self.ensure_space(line_metrics.raise + line_metrics.height);
        let top = self.cursor + line_metrics.raise;
        let baseline = top + ASCENT * size;
        self.paint_line(line, size, self.left(indent), baseline, self.content_width(indent));
        self.cursor = top + line_metrics.height;
    }

    /// Vertical extent of one composed line, grown by any tall formula.
    ///
    /// A formula taller than the text ascent pushes the line top down so the
    /// formula and the text share one baseline without crossing the previous
    /// line; the line box then covers the formula's full vertical extent, depth
    /// included.
    fn inline_line_metrics(&self, line: &[InlineAtom], size: f64) -> LineMetrics {
        let mut formula_height = 0.0f64;
        let mut formula_depth = 0.0f64;
        for atom in line {
            if let InlineAtom::Math(math) = atom {
                formula_height = formula_height.max(math.height);
                formula_depth = formula_depth.max(math.depth);
            }
        }
        let raise = (formula_height - ASCENT * size).max(0.0);
        LineMetrics { raise, height: self.line_box(size).max(raise + ASCENT * size + formula_depth) }
    }

    /// Paint the atoms of one composed line from `x` on one baseline.
    ///
    /// Consecutive atoms sharing a face, a destination, and a hover text stay
    /// one run, and a change of any key starts a new run so emphasis and link
    /// metadata survive into every renderer.
    fn paint_line(&mut self, line: &[InlineAtom], size: f64, x: f64, baseline: f64, max_width: f64) {
        let mut x = x;
        let mut run = PendingText::body();
        let mut run_x = x;
        for atom in line {
            match atom {
                InlineAtom::Text { text, face, href, title } => {
                    if run.is_empty() {
                        run_x = x;
                        run.face = *face;
                        run.href = href.clone();
                        run.title = title.clone();
                    }
                    else if run.face != *face
                        || run.href.as_deref() != href.as_deref()
                        || run.title.as_deref() != title.as_deref()
                    {
                        self.paint_inline_run(&mut run, run_x, baseline, size);
                        run_x = x;
                        run.face = *face;
                        run.href = href.clone();
                        run.title = title.clone();
                    }
                    run.text.push_str(text);
                    x += measure_text(&self.metrics, text, size, *face);
                }
                InlineAtom::Math(math) => {
                    self.paint_inline_run(&mut run, run_x, baseline, size);
                    self.emit_inline_math(math, x, baseline, max_width);
                    x += math.width;
                }
                InlineAtom::Break => {}
            }
        }
        self.paint_inline_run(&mut run, run_x, baseline, size);
    }

    /// Paint the buffered paragraph text as one run and clear the buffer.
    ///
    /// A strikethrough run also emits its decoration as a rule right after the
    /// text run, so paint order keeps the rule above the glyphs. The rule spans
    /// exactly the measured run width through the same face-aware metrics.
    fn paint_inline_run(&mut self, run: &mut PendingText, x: f64, baseline: f64, size: f64) {
        if run.is_empty() {
            return;
        }
        let width = measure_text(&self.metrics, &run.text, size, run.face);
        let struck = run.face.style == FontStyle::Strike;
        self.push(LayoutItem::TextRun(TextRun {
            x,
            y: baseline,
            size,
            width,
            font_family: run.face.family.to_string(),
            style: run.face.style,
            text: std::mem::take(&mut run.text),
            href: run.href.clone(),
            title: run.title.clone(),
        }));
        if struck {
            let y = baseline - STRIKE_OFFSET * size;
            let rule = Rule { x1: x, y1: y, x2: x + width, y2: y, thickness: STRIKE_THICKNESS * size };
            self.push(LayoutItem::Rule(rule));
        }
    }

    /// Paint a formula at the inline baseline, clamped to the content width.
    fn emit_inline_math(&mut self, math: &MathAtom, x: f64, baseline: f64, max_width: f64) {
        let scale = if math.width > max_width && math.width > 0.0 { (max_width / math.width).max(0.0) } else { 1.0 };
        for item in collect_layout(&math.node, x, baseline) {
            self.push(clamp_inline_item(item, x, baseline, scale));
        }
    }

    /// Reserve a fixed box for one authored image.
    ///
    /// The box size is independent of the source, so layout never reads image
    /// bytes. The destination is passed through for renderers that can resolve
    /// a `data:` payload or a local file.
    fn emit_image_placeholder(&mut self, alt: &str, source: &str, indent: f64) {
        let size = self.body_size();
        let width = self.content_width(indent).min(240.0);
        let height = width * 0.625;
        self.pad(size * 0.3);
        self.ensure_space(height);
        let x = self.left(indent);
        let source = (!source.trim().is_empty()).then(|| source.to_string());
        self.push(LayoutItem::ImagePlaceholder(ImagePlaceholder {
            x,
            y: self.cursor,
            width,
            height,
            alt: alt.to_string(),
            source,
        }));
        self.cursor += height;
        self.pad(size * 0.4);
    }

    /// Place the scene the host registered for a `diagram` block.
    ///
    /// The scene keeps its aspect ratio and is scaled down to the content
    /// column when it is wider than the column. A block without a scene, or a
    /// scene without a positive canvas, is dropped instead of being guessed at.
    /// Returns whether a graphic was placed.
    fn emit_diagram(&mut self, id: NodeId, indent: f64) -> bool {
        let Some(diagram) = self.diagrams.get(id)
        else {
            return false;
        };
        let intrinsic = diagram.scene.size;
        if intrinsic.width <= 0.0 || intrinsic.height <= 0.0 {
            return false;
        }
        let size = self.body_size();
        let scale = (self.content_width(indent) / intrinsic.width).min(1.0);
        let width = intrinsic.width * scale;
        let height = intrinsic.height * scale;
        self.pad(size * 0.4);
        self.ensure_space(height);
        let x = self.left(indent);
        self.push(LayoutItem::Graphic(Graphic {
            x,
            y: self.cursor,
            width,
            height,
            scene: diagram.scene.clone(),
            alt: diagram.alt.clone(),
        }));
        self.cursor += height;
        self.pad(size * 0.6);
        true
    }

    fn emit_list(&mut self, ordered: bool, items: &[ListItem], indent: f64, graph: &DocumentGraph) {
        let size = self.body_size();
        let line_box = self.line_box(size);
        for (index, item) in items.iter().enumerate() {
            let marker = match (ordered, item.number) {
                (true, Some(number)) => format!("{number}."),
                (true, None) => format!("{}.", index + 1),
                (false, _) => "•".to_string(),
            };
            let marker_width = measure_text(&self.metrics, &marker, size, BODY_FACE);
            let content_indent = indent + LIST_INDENT.max(marker_width + 6.0);
            let atoms = self.compose_atoms(&item.content, size, content_indent, BODY_FACE, ImageMode::AltText);
            self.pad(size * 0.15);
            self.ensure_space(line_box * 2.0);
            // The marker keeps the baseline of the first content line.
            self.emit_baseline(marker, size, SERIF, indent);
            self.emit_inline_lines(atoms, size, content_indent);
            self.render_blocks(graph, &item.children, content_indent);
            self.pad(size * 0.2);
        }
    }

    fn emit_quote(&mut self, content: &[Inline], indent: f64) {
        let size = self.body_size();
        let quote_indent = indent + 16.0;
        let atoms = self.compose_atoms(content, size, quote_indent, BODY_FACE, ImageMode::AltText);
        let lines = break_inline_atoms(&self.metrics, atoms, size, self.content_width(quote_indent));
        // The rule spans the real line extents, so a tall formula inside the
        // quote grows the rule instead of hanging outside it.
        let height: f64 = lines
            .iter()
            .map(|line| {
                let metrics = self.inline_line_metrics(line, size);
                metrics.raise + metrics.height
            })
            .sum();
        self.pad(size * 0.4);
        self.ensure_space(height);
        let x = self.left(indent) + 2.0;
        let top = self.cursor;
        self.push(LayoutItem::Rule(Rule { x1: x, y1: top, x2: x, y2: top + height, thickness: 2.0 }));
        for line in &lines {
            self.emit_inline_line(line, size, quote_indent);
        }
        self.pad(size * 0.5);
    }

    fn emit_code(&mut self, content: &str, indent: f64) {
        let size = self.body_size() * 0.92;
        let line_box = self.line_box(size) * 0.95;
        let padding = size * 0.6;
        let lines: Vec<&str> = content.lines().collect();
        let height = lines.len().max(1) as f64 * line_box + padding * 2.0;
        self.pad(size * 0.4);
        self.ensure_space(height.min(self.hint.content_height()));
        let top = self.cursor;
        let x = self.left(indent);
        let width = self.content_width(indent);
        self.push(LayoutItem::Rect(Rect { x, y: top, width, height, filled: true }));
        for line in &lines {
            // Code lines stay verbatim: no wrapping, overflow is visible rather than hidden.
            let y = self.cursor + padding + ASCENT * size;
            let width = measure_text(&self.metrics, line, size, CODE_FACE);
            self.push(LayoutItem::TextRun(TextRun {
                x: x + padding,
                y,
                size,
                width,
                font_family: MONOSPACE.to_string(),
                style: FontStyle::Normal,
                text: (*line).to_string(),
                href: None,
                title: None,
            }));
            self.cursor += line_box;
        }
        self.cursor = top + height;
        self.pad(size * 0.6);
    }

    fn emit_table(&mut self, caption: Option<&str>, rows: &[TableRow], indent: f64) {
        let size = self.body_size();
        let line_box = self.line_box(size);
        let x = self.left(indent);
        let width = self.content_width(indent);
        let padding = size * 0.35;
        let columns = rows
            .iter()
            .map(|row| row.cells.iter().map(|cell| cell.colspan.max(1) as usize).sum::<usize>())
            .max()
            .unwrap_or(1)
            .max(1);
        let column_width = width / columns as f64;
        if let Some(caption) = caption {
            self.pad(size * 0.3);
            self.emit_wrapped(caption, size * 0.9, SERIF, indent);
        }
        self.pad(size * 0.4);
        for (row_index, row) in rows.iter().enumerate() {
            let cells: Vec<CellLines> = row
                .cells
                .iter()
                .map(|cell| {
                    let span = cell.colspan.max(1) as usize;
                    let available = (column_width * span as f64 - padding * 2.0).max(24.0);
                    let atoms = self.compose_atoms(&cell.content, size, indent, BODY_FACE, ImageMode::AltText);
                    CellLines { span, available, lines: break_inline_atoms(&self.metrics, atoms, size, available) }
                })
                .collect();
            let max_lines = cells.iter().map(|cell| cell.lines.len()).max().unwrap_or(1).max(1);
            let height = max_lines as f64 * line_box + padding * 2.0;
            self.ensure_space(height);
            let top = self.cursor;
            let thickness = if row.is_header { 1.0 } else { 0.4 };
            self.push(LayoutItem::Rule(Rule { x1: x, y1: top, x2: x + width, y2: top, thickness }));
            let mut column = 0usize;
            for cell in &cells {
                let cell_x = x + column_width * column as f64 + padding;
                let mut line_top = top + padding;
                for line in &cell.lines {
                    self.paint_line(line, size, cell_x, line_top + ASCENT * size, cell.available);
                    line_top += line_box;
                }
                column += cell.span;
            }
            self.cursor = top + height;
            if row_index + 1 == rows.len() {
                self.push(LayoutItem::Rule(Rule { x1: x, y1: self.cursor, x2: x + width, y2: self.cursor, thickness: 0.4 }));
            }
        }
        self.pad(size * 0.8);
    }

    /// Lay out display math and paint its positioned primitives.
    ///
    /// The formula is centred inside the content column like TeX display math
    /// and follows the document body size. When the math engine cannot parse or
    /// lay out the source, the raw TeX is rendered as text: a documented
    /// degradation that keeps the formula visible instead of dropping it.
    fn emit_math(&mut self, content: &str, indent: f64) {
        let size = self.body_size();
        let padding = size * 0.8;
        match layout_formula_with_size(content, true, size) {
            Ok((node, formula)) => {
                self.pad(padding);
                self.ensure_space(formula.height + formula.depth + padding * 2.0);
                let left = self.left(indent);
                let offset = ((self.content_width(indent) - formula.width) / 2.0).max(0.0);
                let baseline = self.cursor + padding + formula.height;
                for item in collect_layout(&node, left + offset, baseline) {
                    self.push(item);
                }
                self.cursor = baseline + formula.depth + padding;
                self.pad(padding);
            }
            Err(_) => self.emit_math_text_fallback(content, indent),
        }
    }

    /// Plain-text rendering used when the math engine rejects the source.
    fn emit_math_text_fallback(&mut self, content: &str, indent: f64) {
        let size = self.body_size();
        self.pad(size * 0.4);
        self.emit_wrapped(content, size, SERIF, indent + 12.0);
        self.pad(size * 0.5);
    }

    fn emit_thematic_break(&mut self, indent: f64) {
        let size = self.body_size();
        self.pad(size * 0.6);
        self.ensure_space(1.0);
        let x = self.left(indent);
        let width = self.content_width(indent);
        let y = self.cursor;
        self.push(LayoutItem::Rule(Rule { x1: x, y1: y, x2: x + width, y2: y, thickness: 0.6 }));
        self.cursor += size * 0.6;
    }

    fn render_blocks(&mut self, graph: &DocumentGraph, ids: &[NodeId], indent: f64) {
        for id in ids {
            let Some(node) = graph.block(*id)
            else {
                continue;
            };
            match &node.block {
                Block::Section { level, title, children } => {
                    self.emit_heading(*level, title, indent);
                    self.render_blocks(graph, children, indent);
                }
                Block::Paragraph { content } => self.emit_paragraph(content, indent),
                Block::List { ordered, items } => {
                    self.emit_list(*ordered, items, indent, graph);
                    self.pad(self.body_size() * 0.4);
                }
                Block::Quote { content } => self.emit_quote(content, indent),
                Block::Code { content, language } => {
                    // A Mermaid fence whose scene the host registered is a
                    // diagram, not preformatted code. A failed or missing
                    // lowering keeps the authored fence text visible.
                    if !is_diagram_language(language.as_deref()) || !self.emit_diagram(*id, indent) {
                        self.emit_code(content, indent);
                    }
                }
                Block::Math { content, language: _ } => self.emit_math(content, indent),
                Block::Table { caption, rows } => self.emit_table(caption.as_deref(), rows, indent),
                Block::Opaque { kind, .. } if kind == "thematic_break" => self.emit_thematic_break(indent),
                // A diagram block and an imported TeX picture environment carry
                // no geometry themselves; the host-supplied scene is the only
                // source of their placement. A picture without a scene is
                // dropped, which matches the previous treatment of the opaque
                // environment.
                Block::Opaque { kind, .. } if kind == "diagram" || kind == "tex.environment.tikzpicture" => {
                    self.emit_diagram(*id, indent);
                }
                // Opaque payloads with no layout meaning are dropped, not guessed.
                Block::Opaque { .. } => {}
            }
        }
    }
}

/// Split text into paragraph atoms: words, single spaces, single CJK glyphs,
/// and hard breaks. This mirrors [`split_atoms`], but keeps authored newlines
/// as forced line endings instead of collapsing them into spaces. Every atom
/// carries `face`, `href`, and `title`, so emphasis and link metadata survive
/// line breaking.
fn split_inline_atoms(text: &str, face: TextFace, href: Option<&str>, title: Option<&str>) -> Vec<InlineAtom> {
    let mut atoms = Vec::new();
    let mut word = String::new();
    let mut in_space = false;
    let mut push_text = |value: String, atoms: &mut Vec<InlineAtom>| {
        atoms.push(InlineAtom::Text { text: value, face, href: href.map(str::to_string), title: title.map(str::to_string) });
    };
    for ch in text.chars() {
        if ch == '\n' {
            if !word.is_empty() {
                push_text(std::mem::take(&mut word), &mut atoms);
            }
            atoms.push(InlineAtom::Break);
            in_space = false;
            continue;
        }
        if ch.is_whitespace() {
            if !word.is_empty() {
                push_text(std::mem::take(&mut word), &mut atoms);
            }
            if !in_space {
                push_text(" ".to_string(), &mut atoms);
                in_space = true;
            }
            continue;
        }
        in_space = false;
        if (ch as u32) >= 0x2E80 {
            if !word.is_empty() {
                push_text(std::mem::take(&mut word), &mut atoms);
            }
            push_text(ch.to_string(), &mut atoms);
            continue;
        }
        word.push(ch);
    }
    if !word.is_empty() {
        push_text(word, &mut atoms);
    }
    atoms
}

/// Greedy first-fit line breaking over paragraph atoms.
///
/// Formula atoms are never split; one that does not fit starts the next line,
/// and one wider than the whole column occupies a line of its own.
fn break_inline_atoms(metrics: &FontMetricsSystem, atoms: Vec<InlineAtom>, size: f64, max_width: f64) -> Vec<Vec<InlineAtom>> {
    let mut lines: Vec<Vec<InlineAtom>> = Vec::new();
    let mut current: Vec<InlineAtom> = Vec::new();
    let mut width = 0.0f64;
    for atom in atoms {
        match atom {
            InlineAtom::Break => finish_inline_line(&mut lines, &mut current, &mut width),
            InlineAtom::Text { text, face, href, title } => {
                let atom_width = measure_text(metrics, &text, size, face);
                if text.trim().is_empty() {
                    // A space never starts a line and never widens past the column.
                    if !current.is_empty() && width + atom_width <= max_width {
                        current.push(InlineAtom::Text { text, face, href, title });
                        width += atom_width;
                    }
                    continue;
                }
                if !current.is_empty() && width + atom_width > max_width {
                    finish_inline_line(&mut lines, &mut current, &mut width);
                }
                if atom_width > max_width && current.is_empty() {
                    // Break an oversized word or URL character by character.
                    let mut chunk = String::new();
                    let mut chunk_width = 0.0;
                    for ch in text.chars() {
                        let advance = advance_em(metrics, ch, face) * size;
                        if !chunk.is_empty() && chunk_width + advance > max_width {
                            lines.push(vec![InlineAtom::Text {
                                text: chunk.trim_end().to_string(),
                                face,
                                href: href.clone(),
                                title: title.clone(),
                            }]);
                            chunk.clear();
                            chunk_width = 0.0;
                        }
                        chunk.push(ch);
                        chunk_width += advance;
                    }
                    current.push(InlineAtom::Text { text: chunk, face, href, title });
                    width = chunk_width;
                    continue;
                }
                current.push(InlineAtom::Text { text, face, href, title });
                width += atom_width;
            }
            InlineAtom::Math(math) => {
                if !current.is_empty() && width + math.width > max_width {
                    finish_inline_line(&mut lines, &mut current, &mut width);
                }
                let oversized = math.width > max_width;
                width += math.width;
                current.push(InlineAtom::Math(math));
                if oversized {
                    // A formula wider than the column never shares its line.
                    finish_inline_line(&mut lines, &mut current, &mut width);
                }
            }
        }
    }
    finish_inline_line(&mut lines, &mut current, &mut width);
    lines
}

/// Close one line: drop trailing spaces and keep it only when it has content.
fn finish_inline_line(lines: &mut Vec<Vec<InlineAtom>>, current: &mut Vec<InlineAtom>, width: &mut f64) {
    while matches!(current.last(), Some(InlineAtom::Text { text, .. }) if text.trim().is_empty()) {
        current.pop();
    }
    if !current.is_empty() {
        lines.push(std::mem::take(current));
    }
    else {
        current.clear();
    }
    *width = 0.0;
}

/// Scale an inline formula primitive about its origin and baseline.
///
/// Used only when a formula is wider than the content column: scaling keeps
/// every glyph and rule inside the column instead of dropping the formula.
fn clamp_inline_item(item: LayoutItem, origin_x: f64, baseline: f64, scale: f64) -> LayoutItem {
    if (scale - 1.0).abs() < f64::EPSILON {
        return item;
    }
    match item {
        LayoutItem::TextRun(run) => LayoutItem::TextRun(TextRun {
            x: origin_x + (run.x - origin_x) * scale,
            y: baseline + (run.y - baseline) * scale,
            size: run.size * scale,
            width: run.width * scale,
            font_family: run.font_family,
            style: run.style,
            text: run.text,
            href: run.href,
            title: run.title,
        }),
        LayoutItem::Rule(rule) => LayoutItem::Rule(Rule {
            x1: origin_x + (rule.x1 - origin_x) * scale,
            y1: baseline + (rule.y1 - baseline) * scale,
            x2: origin_x + (rule.x2 - origin_x) * scale,
            y2: baseline + (rule.y2 - baseline) * scale,
            thickness: rule.thickness * scale,
        }),
        other => other,
    }
}

fn blank_page(hint: &LayoutHint) -> LayoutPage {
    LayoutPage { width: hint.page_width, height: hint.page_height, items: Vec::new() }
}

/// Sum of character advances for a run of text, in points.
fn measure_text(metrics: &FontMetricsSystem, text: &str, size: f64, face: TextFace) -> f64 {
    text.chars().map(|ch| advance_em(metrics, ch, face) * size).sum()
}

/// Advance width of one character in em units under `face`.
///
/// `vutex-text` currently exposes a uniform 0.5 em advance for known glyphs, so
/// the character class refines that base advance for book-text line breaking.
/// Monospace text gets a fixed advance, so positioned runs line up with the
/// monospace font the renderers actually select.
fn advance_em(metrics: &FontMetricsSystem, ch: char, face: TextFace) -> f64 {
    if face.family == MONOSPACE {
        return MONOSPACE_ADVANCE;
    }
    if ch.is_whitespace() {
        return 0.25;
    }
    let key = GlyphKey::from_char(ch, Some("default".to_string()), FontStyle::Normal);
    let base = metrics.get_metrics(&key).map(|glyph| glyph.width.to_f64()).unwrap_or(0.5);
    match ch {
        'i' | 'l' | 'j' | 't' | 'f' | 'r' | 'I' | '.' | ',' | '\'' | '!' | '|' | ':' | ';' | '(' | ')' | '[' | ']' => {
            base * 0.62
        }
        'W' | 'M' | 'm' | 'w' | '@' => base * 1.7,
        'A'..='Z' => base * 1.25,
        '0'..='9' => base * 1.1,
        ch if (ch as u32) >= 0x2E80 => 1.0,
        _ => base,
    }
}

/// Split text into breakable atoms: words, single spaces, and single CJK glyphs.
fn split_atoms(text: &str) -> Vec<String> {
    let mut atoms = Vec::new();
    let mut word = String::new();
    let mut in_space = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            if !word.is_empty() {
                atoms.push(std::mem::take(&mut word));
            }
            if !in_space {
                atoms.push(" ".to_string());
                in_space = true;
            }
            continue;
        }
        in_space = false;
        if (ch as u32) >= 0x2E80 {
            if !word.is_empty() {
                atoms.push(std::mem::take(&mut word));
            }
            atoms.push(ch.to_string());
            continue;
        }
        word.push(ch);
    }
    if !word.is_empty() {
        atoms.push(word);
    }
    atoms
}

/// Greedy first-fit line breaking with word and CJK break opportunities.
fn wrap_text(metrics: &FontMetricsSystem, text: &str, size: f64, max_width: f64) -> Vec<String> {
    let mut lines = Vec::new();
    for raw in text.split('\n') {
        let mut line = String::new();
        let mut width = 0.0;
        for atom in split_atoms(raw) {
            let atom_width = measure_text(metrics, &atom, size, BODY_FACE);
            if atom.trim().is_empty() {
                if line.is_empty() || width + atom_width > max_width {
                    continue;
                }
                line.push(' ');
                width += atom_width;
                continue;
            }
            if !line.is_empty() && width + atom_width > max_width {
                lines.push(line.trim_end().to_string());
                line = String::new();
                width = 0.0;
            }
            if atom_width > max_width && line.is_empty() {
                // Break an oversized word or URL character by character.
                let mut chunk = String::new();
                let mut chunk_width = 0.0;
                for ch in atom.chars() {
                    let advance = advance_em(metrics, ch, BODY_FACE) * size;
                    if !chunk.is_empty() && chunk_width + advance > max_width {
                        lines.push(chunk.trim_end().to_string());
                        chunk = String::new();
                        chunk_width = 0.0;
                    }
                    chunk.push(ch);
                    chunk_width += advance;
                }
                line = chunk;
                width = chunk_width;
                continue;
            }
            line.push_str(&atom);
            width += atom_width;
        }
        if !line.trim().is_empty() {
            lines.push(line.trim_end().to_string());
        }
    }
    lines
}
