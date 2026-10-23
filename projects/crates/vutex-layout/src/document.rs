//! Document layout pass: Notedown block tree to positioned page primitives.
//!
//! The pass walks [`DocumentGraph`] blocks in reading order, measures inline
//! content through `vutex-text` metrics, wraps lines greedily inside the
//! content width, and paginates at line granularity.

use std::collections::HashSet;

use notedown_ir::{Block, DocumentGraph, Inline, ListItem, NodeId, TableRow};
use vutex_text::math::{FontMetricsSystem, builtin_font_metrics};
use vutex_types::math::{FontStyle, GlyphKey};

use crate::{
    hint::LayoutHint,
    math::{collect_layout, layout_formula_with_size},
    plan::{ImagePlaceholder, LayoutItem, LayoutPage, LayoutPlan, Rect, Rule, TextRun},
};

/// Text baseline height as a fraction of the font size.
const ASCENT: f64 = 0.8;
/// Horizontal indent per list nesting level, in points.
const LIST_INDENT: f64 = 18.0;
/// Font family used for body text.
const SERIF: &str = "serif";
/// Font family used for code and preformatted text.
const MONOSPACE: &str = "monospace";

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
    let mut builder = PageBuilder::new(*hint);
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

/// Page-filling state for one layout pass.
struct PageBuilder {
    hint: LayoutHint,
    metrics: FontMetricsSystem,
    pages: Vec<LayoutPage>,
    /// Top edge of the next line box, in page coordinates.
    cursor: f64,
}

impl PageBuilder {
    fn new(hint: LayoutHint) -> Self {
        Self { hint, metrics: builtin_font_metrics(), pages: vec![blank_page(&hint)], cursor: hint.margin.top }
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
        self.push(LayoutItem::TextRun(TextRun { x, y, size, font_family: family.to_string(), text }));
    }

    fn emit_wrapped(&mut self, text: &str, size: f64, family: &str, indent: f64) {
        for line in self.wrap(text, size, indent) {
            if line.is_empty() {
                continue;
            }
            self.emit_line(line, size, family, indent);
        }
    }

    fn emit_heading(&mut self, level: u8, title: &str, indent: f64) {
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
        self.emit_wrapped(title, size, SERIF, indent);
        self.pad(base * 0.4);
    }

    fn emit_paragraph(&mut self, content: &[Inline], indent: f64) {
        let size = self.body_size();
        let mut buffer = String::new();
        for inline in content {
            match inline {
                Inline::Image { alt, .. } => {
                    let text = buffer.trim().to_string();
                    buffer.clear();
                    if !text.is_empty() {
                        self.emit_wrapped(&text, size, SERIF, indent);
                    }
                    let alt_text: String = alt.iter().map(Inline::plain_text).collect();
                    self.emit_image_placeholder(&alt_text, indent);
                }
                other => buffer.push_str(&other.plain_text()),
            }
        }
        let text = buffer.trim().to_string();
        if !text.is_empty() {
            self.emit_wrapped(&text, size, SERIF, indent);
        }
        self.pad(size * 0.6);
    }

    fn emit_image_placeholder(&mut self, alt: &str, indent: f64) {
        let size = self.body_size();
        let width = self.content_width(indent).min(240.0);
        let height = width * 0.625;
        self.pad(size * 0.3);
        self.ensure_space(height);
        let x = self.left(indent);
        self.push(LayoutItem::ImagePlaceholder(ImagePlaceholder { x, y: self.cursor, width, height, alt: alt.to_string() }));
        self.cursor += height;
        self.pad(size * 0.4);
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
            let marker_width = measure_text(&self.metrics, &marker, size);
            let content_indent = indent + LIST_INDENT.max(marker_width + 6.0);
            let text: String = item.content.iter().map(Inline::plain_text).collect();
            self.pad(size * 0.15);
            self.ensure_space(line_box * 2.0);
            self.emit_baseline(marker, size, SERIF, indent);
            self.emit_wrapped(&text, size, SERIF, content_indent);
            self.render_blocks(graph, &item.children, content_indent);
            self.pad(size * 0.2);
        }
    }

    fn emit_quote(&mut self, content: &[Inline], indent: f64) {
        let size = self.body_size();
        let text: String = content.iter().map(Inline::plain_text).collect();
        let quote_indent = indent + 16.0;
        let line_box = self.line_box(size);
        let lines = self.wrap(&text, size, quote_indent);
        let height = line_box * lines.len() as f64;
        self.pad(size * 0.4);
        self.ensure_space(height);
        let x = self.left(indent) + 2.0;
        let top = self.cursor;
        self.push(LayoutItem::Rule(Rule { x1: x, y1: top, x2: x, y2: top + height, thickness: 2.0 }));
        for line in lines {
            if line.is_empty() {
                continue;
            }
            self.emit_line(line, size, SERIF, quote_indent);
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
            self.push(LayoutItem::TextRun(TextRun {
                x: x + padding,
                y,
                size,
                font_family: MONOSPACE.to_string(),
                text: (*line).to_string(),
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
            let cells: Vec<(usize, Vec<String>)> = row
                .cells
                .iter()
                .map(|cell| {
                    let span = cell.colspan.max(1) as usize;
                    let text: String = cell.content.iter().map(Inline::plain_text).collect();
                    let available = (column_width * span as f64 - padding * 2.0).max(24.0);
                    (span, wrap_text(&self.metrics, &text, size, available))
                })
                .collect();
            let max_lines = cells.iter().map(|(_, lines)| lines.len()).max().unwrap_or(1).max(1);
            let height = max_lines as f64 * line_box + padding * 2.0;
            self.ensure_space(height);
            let top = self.cursor;
            let thickness = if row.is_header { 1.0 } else { 0.4 };
            self.push(LayoutItem::Rule(Rule { x1: x, y1: top, x2: x + width, y2: top, thickness }));
            let mut column = 0usize;
            for (span, lines) in &cells {
                let cell_x = x + column_width * column as f64 + padding;
                let mut line_top = top + padding;
                for line in lines {
                    if !line.is_empty() {
                        let y = line_top + ASCENT * size;
                        self.push(LayoutItem::TextRun(TextRun {
                            x: cell_x,
                            y,
                            size,
                            font_family: SERIF.to_string(),
                            text: line.clone(),
                        }));
                    }
                    line_top += line_box;
                }
                column += span;
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
                    let heading: String = title.iter().map(Inline::plain_text).collect();
                    self.emit_heading(*level, heading.trim(), indent);
                    self.render_blocks(graph, children, indent);
                }
                Block::Paragraph { content } => self.emit_paragraph(content, indent),
                Block::List { ordered, items } => {
                    self.emit_list(*ordered, items, indent, graph);
                    self.pad(self.body_size() * 0.4);
                }
                Block::Quote { content } => self.emit_quote(content, indent),
                Block::Code { content, language: _ } => self.emit_code(content, indent),
                Block::Math { content, language: _ } => self.emit_math(content, indent),
                Block::Table { caption, rows } => self.emit_table(caption.as_deref(), rows, indent),
                Block::Opaque { kind, .. } if kind == "thematic_break" => self.emit_thematic_break(indent),
                // Opaque payloads with no layout meaning are dropped, not guessed.
                Block::Opaque { .. } => {}
            }
        }
    }
}

fn blank_page(hint: &LayoutHint) -> LayoutPage {
    LayoutPage { width: hint.page_width, height: hint.page_height, items: Vec::new() }
}

/// Sum of character advances for a run of text, in points.
fn measure_text(metrics: &FontMetricsSystem, text: &str, size: f64) -> f64 {
    text.chars().map(|ch| advance_em(metrics, ch) * size).sum()
}

/// Advance width of one character in em units.
///
/// `vutex-text` currently exposes a uniform 0.5 em advance for known glyphs, so
/// the character class refines that base advance for book-text line breaking.
fn advance_em(metrics: &FontMetricsSystem, ch: char) -> f64 {
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
            let atom_width = measure_text(metrics, &atom, size);
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
                    let advance = advance_em(metrics, ch) * size;
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
