//! Lowering from computed layout and document semantics into presentation IR.

use std::collections::HashSet;

use notedown_ir::{Block, DocumentGraph, Inline, NodeId, RelationEndpoint};
use vutex_layout::plan::{Graphic, ImagePlaceholder, LayoutItem, LayoutPlan, Rect, Rule, TextRun};
use vutex_types::math::FontStyle;

use crate::{
    display::{
        DisplayGraphic, DisplayImage, DisplayItem, DisplayPage, DisplayPresentation, DisplayRect, DisplayRule, DisplayTextRun,
    },
    flow::{FlowBlock, FlowListItem, FlowPresentation, FlowRun, FlowTableRow},
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
        LayoutItem::TextRun(TextRun { x, y, size, width, font_family, style, text, href, title }) => {
            DisplayItem::TextRun(DisplayTextRun {
                x: *x,
                y: *y,
                size: *size,
                width: *width,
                font_family: font_family.clone(),
                style: *style,
                text: text.clone(),
                href: href.clone(),
                title: title.clone(),
            })
        }
        LayoutItem::Rule(Rule { x1, y1, x2, y2, thickness }) => {
            DisplayItem::Rule(DisplayRule { x1: *x1, y1: *y1, x2: *x2, y2: *y2, thickness: *thickness })
        }
        LayoutItem::Rect(Rect { x, y, width, height, filled }) => {
            DisplayItem::Rect(DisplayRect { x: *x, y: *y, width: *width, height: *height, filled: *filled })
        }
        LayoutItem::ImagePlaceholder(ImagePlaceholder { x, y, width, height, alt, source }) => {
            DisplayItem::Image(DisplayImage {
                x: *x,
                y: *y,
                width: *width,
                height: *height,
                alt: alt.clone(),
                source: source.clone(),
            })
        }
        LayoutItem::Graphic(Graphic { x, y, width, height, scene, alt }) => DisplayItem::Graphic(DisplayGraphic {
            x: *x,
            y: *y,
            width: *width,
            height: *height,
            scene: scene.clone(),
            alt: alt.clone(),
        }),
    }
}

/// Build the semantic flow presentation of a Notedown document graph.
///
/// The flow keeps block semantics for reflow targets. Nested section children
/// follow their heading, and nested list children stay on their item so no
/// block content is dropped or promoted out of its parent list item.
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
        "strike" => FontStyle::Strike,
        _ => FontStyle::Normal,
    }
}

/// Append one run, merging it into the previous run of the same style, link,
/// and link title.
///
/// A change of destination or hover text ends the previous run, so a link
/// boundary is never merged away and neighbouring anchors with different
/// tooltips keep their own titles.
fn push_run(runs: &mut Vec<FlowRun>, text: &str, style: FontStyle, href: Option<&str>, title: Option<&str>) {
    if text.is_empty() {
        return;
    }
    if let Some(last) = runs.last_mut()
        && last.style == style
        && last.href.as_deref() == href
        && last.title.as_deref() == title
    {
        last.text.push_str(text);
        return;
    }
    runs.push(FlowRun { text: text.to_string(), style, href: href.map(str::to_string), title: title.map(str::to_string) });
}

/// Resolve the destination of a link target.
///
/// Only authored external URIs become destinations. Internal `Node`,
/// `Document`, `Asset`, and `Unresolved` targets keep the display text but stay
/// without a destination, because reflow HTML emits no matching ids and a
/// fabricated `#` anchor or invented URL scheme would point nowhere.
fn link_href(target: &RelationEndpoint) -> Option<&str> {
    match target {
        RelationEndpoint::ExternalUri(uri) if !uri.is_empty() => Some(uri),
        _ => None,
    }
}

/// Resolve the hover text of a link target.
///
/// An authored empty title carries no tooltip, so it lowers to `None` instead of
/// an empty attribute in the output.
fn link_title(title: Option<&String>) -> Option<&str> {
    title.map(String::as_str).filter(|value| !value.is_empty())
}

/// Flatten inline content into styled runs under the enclosing style and link.
fn flow_runs<'a>(
    inlines: &'a [Inline],
    style: FontStyle,
    href: Option<&'a str>,
    title: Option<&'a str>,
    runs: &mut Vec<FlowRun>,
) {
    for inline in inlines {
        match inline {
            Inline::Styled { style: name, children } => flow_runs(children, flow_style(name), href, title, runs),
            Inline::InlineCode { text } => push_run(runs, text, FontStyle::Monospace, href, title),
            Inline::Image { alt, .. } => flow_runs(alt, style, href, title, runs),
            // Attributes and link destinations carry no printable face, but
            // their wrapped content keeps every authored style.
            Inline::Attributed { children, .. } => flow_runs(children, style, href, title, runs),
            Inline::Link { target, display, title: link_hover } => {
                // A nested external link wins. A nested internal link keeps the
                // enclosing destination so its text stays inside the anchor.
                flow_runs(display, style, link_href(target).or(href), link_title(link_hover.as_ref()).or(title), runs);
            }
            Inline::Note { display: Some(display), .. } | Inline::Citation { display: Some(display), .. } => {
                flow_runs(display, style, href, title, runs);
            }
            other => push_run(runs, &other.plain_text(), style, href, title),
        }
    }
}

/// Styled runs for one inline sequence with no enclosing emphasis.
fn runs_of(inlines: &[Inline]) -> Vec<FlowRun> {
    let mut runs = Vec::new();
    flow_runs(inlines, FontStyle::Normal, None, None, &mut runs);
    runs
}

fn collect_flow(graph: &DocumentGraph, ids: &[NodeId], blocks: &mut Vec<FlowBlock>) {
    for id in ids {
        let Some(node) = graph.block(*id)
        else {
            continue;
        };
        match &node.block {
            Block::Section { level, title, children } => {
                blocks.push(FlowBlock::Heading { level: (*level).clamp(1, 6), runs: runs_of(title) });
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
                    flow_runs(std::slice::from_ref(inline), FontStyle::Normal, None, None, &mut runs);
                }
                blocks.push(FlowBlock::Paragraph { runs });
            }
            Block::List { ordered, items } => {
                let items = items
                    .iter()
                    .map(|item| {
                        // Nested blocks stay on the item that authored them, so a
                        // child list renders inside the parent `<li>`.
                        let mut children = Vec::new();
                        collect_flow(graph, &item.children, &mut children);
                        FlowListItem { runs: runs_of(&item.content), number: item.number, children }
                    })
                    .collect();
                blocks.push(FlowBlock::List { ordered: *ordered, items });
            }
            Block::Code { language, content } => {
                blocks.push(FlowBlock::CodeBlock { language: language.clone(), text: content.clone() });
            }
            // Reflow HTML has no scene surface, so the authored diagram source
            // stays visible under its language tag instead of disappearing.
            Block::Diagram { language, source } => {
                blocks.push(FlowBlock::CodeBlock { language: Some(language.clone()), text: source.clone() });
            }
            Block::Math { content, .. } => {
                blocks.push(FlowBlock::CodeBlock { language: None, text: content.clone() });
            }
            Block::Quote { content } => blocks.push(FlowBlock::Quote { runs: runs_of(content) }),
            Block::Table { caption, rows } => {
                blocks.push(FlowBlock::Table {
                    caption: caption.clone(),
                    rows: rows
                        .iter()
                        .map(|row| FlowTableRow {
                            is_header: row.is_header,
                            cells: row.cells.iter().map(|cell| runs_of(&cell.content)).collect(),
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
