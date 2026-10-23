//! Lowering from computed layout and document semantics into presentation IR.

use std::collections::HashSet;

use notedown_ir::{Block, DocumentGraph, Inline, NodeId};
use vutex_layout::plan::{ImagePlaceholder, LayoutItem, LayoutPlan, Rect, Rule, TextRun};
use vutex_types::math::FontStyle;

use crate::{
    display::{DisplayImage, DisplayItem, DisplayPage, DisplayPresentation, DisplayRect, DisplayRule, DisplayTextRun},
    flow::{FlowBlock, FlowPresentation, FlowRun, FlowTableRow},
};

/// Lower a layout plan into a display presentation.
pub fn build_display_presentation(plan: &LayoutPlan) -> DisplayPresentation {
    DisplayPresentation {
        pages: plan
            .pages
            .iter()
            .map(|page| DisplayPage {
                width: page.width,
                height: page.height,
                items: page.items.iter().map(lower_item).collect(),
            })
            .collect(),
    }
}

fn lower_item(item: &LayoutItem) -> DisplayItem {
    match item {
        LayoutItem::TextRun(TextRun { x, y, size, font_family, style, text }) => DisplayItem::TextRun(DisplayTextRun {
            x: *x,
            y: *y,
            size: *size,
            font_family: font_family.clone(),
            style: *style,
            text: text.clone(),
        }),
        LayoutItem::Rule(Rule { x1, y1, x2, y2, thickness }) => {
            DisplayItem::Rule(DisplayRule { x1: *x1, y1: *y1, x2: *x2, y2: *y2, thickness: *thickness })
        }
        LayoutItem::Rect(Rect { x, y, width, height, filled }) => {
            DisplayItem::Rect(DisplayRect { x: *x, y: *y, width: *width, height: *height, filled: *filled })
        }
        LayoutItem::ImagePlaceholder(ImagePlaceholder { x, y, width, height, alt }) => {
            DisplayItem::Image(DisplayImage { x: *x, y: *y, width: *width, height: *height, alt: alt.clone() })
        }
    }
}

/// Build the semantic flow presentation of a Notedown document graph.
///
/// The flow keeps block semantics for reflow targets. Nested section children
/// follow their heading, and nested list children follow their list, so no
/// block content is dropped.
pub fn build_flow_presentation(graph: &DocumentGraph) -> FlowPresentation {
    let mut blocks = Vec::new();
    collect_flow(graph, &root_ids(graph), &mut blocks);
    FlowPresentation { root_tag: "article".to_string(), blocks }
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

fn plain(inlines: &[Inline]) -> String {
    inlines.iter().map(Inline::plain_text).collect()
}

/// Map an inline style name onto the shared face style.
///
/// Unrecognised names fall back to [`FontStyle::Normal`], matching layout: an
/// unknown style string never becomes an invented emphasis.
fn flow_style(style: &str) -> FontStyle {
    match style.to_ascii_lowercase().as_str() {
        "bold" | "strong" => FontStyle::Bold,
        "italic" | "em" => FontStyle::Italic,
        "mono" | "code" => FontStyle::Monospace,
        _ => FontStyle::Normal,
    }
}

/// Append one run, merging it into the previous run of the same style.
fn push_run(runs: &mut Vec<FlowRun>, text: &str, style: FontStyle) {
    if text.is_empty() {
        return;
    }
    if let Some(last) = runs.last_mut()
        && last.style == style
    {
        last.text.push_str(text);
        return;
    }
    runs.push(FlowRun { text: text.to_string(), style });
}

/// Flatten inline content into styled runs under the enclosing style.
fn flow_runs(inlines: &[Inline], style: FontStyle, runs: &mut Vec<FlowRun>) {
    for inline in inlines {
        match inline {
            Inline::Styled { style: name, children } => flow_runs(children, flow_style(name), runs),
            Inline::InlineCode { text } => push_run(runs, text, FontStyle::Monospace),
            Inline::Image { alt, .. } => flow_runs(alt, style, runs),
            other => push_run(runs, &other.plain_text(), style),
        }
    }
}

fn collect_flow(graph: &DocumentGraph, ids: &[NodeId], blocks: &mut Vec<FlowBlock>) {
    for id in ids {
        let Some(node) = graph.block(*id)
        else {
            continue;
        };
        match &node.block {
            Block::Section { level, title, children } => {
                blocks.push(FlowBlock::Heading { level: (*level).clamp(1, 6), text: plain(title) });
                collect_flow(graph, children, blocks);
            }
            Block::Paragraph { content } => {
                let mut runs: Vec<FlowRun> = Vec::new();
                for inline in content {
                    if let Inline::Image { source, alt, .. } = inline {
                        // An inline image ends the run so the image block keeps
                        // its place in document order.
                        if runs.iter().any(|run| !run.text.trim().is_empty()) {
                            blocks.push(FlowBlock::Paragraph { runs: std::mem::take(&mut runs) });
                        }
                        else {
                            runs.clear();
                        }
                        blocks.push(FlowBlock::Image { alt: plain(alt), source: source.clone() });
                        continue;
                    }
                    flow_runs(std::slice::from_ref(inline), FontStyle::Normal, &mut runs);
                }
                blocks.push(FlowBlock::Paragraph { runs });
            }
            Block::List { ordered, items } => {
                blocks.push(FlowBlock::List {
                    ordered: *ordered,
                    items: items.iter().map(|item| plain(&item.content)).collect(),
                });
                for item in items {
                    collect_flow(graph, &item.children, blocks);
                }
            }
            Block::Code { language, content } => {
                blocks.push(FlowBlock::CodeBlock { language: language.clone(), text: content.clone() });
            }
            Block::Math { content, .. } => {
                blocks.push(FlowBlock::CodeBlock { language: None, text: content.clone() });
            }
            Block::Quote { content } => blocks.push(FlowBlock::Quote { text: plain(content) }),
            Block::Table { caption, rows } => {
                blocks.push(FlowBlock::Table {
                    caption: caption.clone(),
                    rows: rows
                        .iter()
                        .map(|row| FlowTableRow {
                            is_header: row.is_header,
                            cells: row.cells.iter().map(|cell| plain(&cell.content)).collect(),
                        })
                        .collect(),
                });
            }
            Block::Opaque { kind, .. } if kind == "thematic_break" => blocks.push(FlowBlock::Rule),
            // Opaque payloads with no flow meaning are dropped, not guessed.
            Block::Opaque { .. } => {}
        }
    }
}
