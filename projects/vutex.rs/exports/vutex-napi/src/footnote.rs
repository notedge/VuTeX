//! GFM footnote apparatus for the host layer.
//!
//! The input contract carries GFM footnotes as two separate semantic nodes:
//! every reference is an [`Inline::Note`], and every authored definition is one
//! `Block::Opaque { kind: "footnote_definition" }` whose payload keeps the
//! original `[^label]: body` text. The IR has no footnote collection, no
//! numbering, and no placement rule, so a paginated consumer that stops at the
//! contract renders `[^1]` literally and drops every definition.
//!
//! This module projects exactly those two contract nodes into layout-ready
//! content without inventing a second footnote syntax:
//!
//! * each resolved reference becomes a numbered marker carried by a text run,
//!   so every back end paints it without needing a superscript run,
//! * every referenced definition body moves into one endnote area that closes
//!   the document,
//! * unresolved, repeated, duplicated, and unreferenced labels stay visible as
//!   structured diagnostics instead of being dropped silently.

use std::collections::BTreeMap;

use document_ir::{Block, DocumentGraph, Inline, NodeId, SemanticStatus};

use crate::document::{DiagnosticRecord, DiagnosticSpan};

/// Diagnostic code for a reference whose label has no definition.
pub const FOOTNOTE_DEFINITION_MISSING: &str = "VUTEX_FOOTNOTE_DEFINITION_MISSING";
/// Diagnostic code for a second definition of a label already defined.
pub const FOOTNOTE_DEFINITION_DUPLICATE: &str = "VUTEX_FOOTNOTE_DEFINITION_DUPLICATE";
/// Diagnostic code for a definition payload the GFM grammar cannot read.
pub const FOOTNOTE_DEFINITION_UNPARSED: &str = "VUTEX_FOOTNOTE_DEFINITION_UNPARSED";
/// Diagnostic code for a definition label that no reference uses.
pub const FOOTNOTE_DEFINITION_UNREFERENCED: &str = "VUTEX_FOOTNOTE_DEFINITION_UNREFERENCED";
/// Diagnostic code for a label referenced more than once.
pub const FOOTNOTE_REFERENCE_REPEATED: &str = "VUTEX_FOOTNOTE_REFERENCE_REPEATED";

/// Pipeline stage stamped on every footnote diagnostic.
const STAGE: &str = "footnote";
/// IR opaque kind that keeps one authored GFM footnote definition.
const DEFINITION_KIND: &str = "footnote_definition";
/// IR opaque kind the layout layer paints as a horizontal rule.
const THEMATIC_BREAK_KIND: &str = "thematic_break";

/// One authored footnote definition read from the input contract.
struct Definition {
    /// Node that carries the authored definition block.
    id: NodeId,
    /// GFM label without the `[^` prefix and `]` suffix.
    label: String,
    /// Plain body text the definition carries.
    body: String,
}

/// Graph plus diagnostics produced by the footnote projection.
pub struct FootnoteLowering {
    /// Graph whose references and definitions are replaced by laid-out content.
    pub graph: DocumentGraph,
    /// Structured diagnostics for every anomaly the projection met.
    pub diagnostics: Vec<DiagnosticRecord>,
    /// Number of references the contract carried.
    pub references: usize,
    /// Number of authored definitions the contract carried.
    pub definitions: usize,
    /// Number of notes that reached the endnote area.
    pub notes: usize,
}

/// Project GFM footnote references and definitions into layout-ready content.
///
/// The caller keeps the imported graph for structural reports and lays out the
/// returned graph instead.
pub fn lower_footnotes(graph: &DocumentGraph, path: &str) -> FootnoteLowering {
    let mut diagnostics = Vec::new();
    let mut definitions: Vec<Definition> = Vec::new();
    let mut removed = Vec::new();
    let mut defined: BTreeMap<String, usize> = BTreeMap::new();

    for node in &graph.blocks {
        let Block::Opaque { kind, payload_hint, .. } = &node.block
        else {
            continue;
        };
        if kind != DEFINITION_KIND {
            continue;
        }
        let Some((label, body)) = parse_definition(payload_hint)
        else {
            diagnostics.push(record(
                FOOTNOTE_DEFINITION_UNPARSED,
                "warning",
                format!("footnote definition is not readable as `[^label]: body`: `{}`", payload_hint.trim()),
                Some(node.id),
                graph,
                path,
            ));
            continue;
        };
        if let Some(&first) = defined.get(&label) {
            let first_label = definitions[first].label.clone();
            diagnostics.push(record(
                FOOTNOTE_DEFINITION_DUPLICATE,
                "warning",
                format!(
                    "footnote definition `[^{label}]` repeats the definition already given for `[^{first_label}]`, so only the first body stays"
                ),
                Some(node.id),
                graph,
                path,
            ));
            removed.push(node.id);
            continue;
        }
        defined.insert(label.clone(), definitions.len());
        definitions.push(Definition { id: node.id, label, body });
        removed.push(node.id);
    }

    let definitions_count = definitions.len();

    let mut order: Vec<String> = Vec::new();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for node in &graph.blocks {
        visit_block_inlines(&node.block, &mut |inline| {
            if let Inline::Note { label, display: None } = inline {
                let count = counts.entry(label.clone()).or_insert(0);
                *count += 1;
                if *count == 1 {
                    order.push(label.clone());
                }
            }
        });
    }
    let references = counts.values().sum::<usize>();

    let mut numbering: BTreeMap<String, usize> = BTreeMap::new();
    let mut missing: Vec<String> = Vec::new();
    for label in &order {
        if defined.contains_key(label) {
            let number = numbering.len() + 1;
            numbering.insert(label.clone(), number);
        }
        else {
            missing.push(label.clone());
        }
    }
    for label in &missing {
        diagnostics.push(record(
            FOOTNOTE_DEFINITION_MISSING,
            "warning",
            format!("footnote reference `[^{label}]` has no definition, so the marker stays literal"),
            None,
            graph,
            path,
        ));
    }
    for label in &order {
        let count = counts.get(label).copied().unwrap_or(0);
        if count > 1 {
            let resolved = numbering.get(label).copied();
            diagnostics.push(record(
                FOOTNOTE_REFERENCE_REPEATED,
                "info",
                match resolved {
                    Some(number) => {
                        format!("footnote `[^{label}]` is referenced {count} times, so all markers reuse number {number}")
                    }
                    None => format!("footnote `[^{label}]` is referenced {count} times without a definition"),
                },
                None,
                graph,
                path,
            ));
        }
    }
    for definition in &definitions {
        if counts.get(&definition.label).copied().unwrap_or(0) == 0 {
            diagnostics.push(record(
                FOOTNOTE_DEFINITION_UNREFERENCED,
                "info",
                format!(
                    "footnote definition `[^{}]` is never referenced, so its body stays out of the endnote area",
                    definition.label
                ),
                Some(definition.id),
                graph,
                path,
            ));
        }
    }

    let mut rewritten = graph.clone();
    for id in &removed {
        let _ = rewritten.delete_block(*id);
    }
    for node in &mut rewritten.blocks {
        for_each_block_inline(&mut node.block, &mut |inline| {
            if let Inline::Note { label, display: None } = inline {
                let text = match numbering.get(label) {
                    Some(&number) => format!("[{number}]"),
                    None => format!("[^{label}]"),
                };
                *inline = Inline::Text { text };
            }
        });
    }

    let mut notes = Vec::new();
    for label in &order {
        let Some(&number) = numbering.get(label)
        else {
            continue;
        };
        let Some(&index) = defined.get(label)
        else {
            continue;
        };
        let definition = &definitions[index];
        notes.push((number, definition.body.clone()));
    }
    notes.sort_by_key(|(number, _)| *number);

    if !notes.is_empty() {
        rewritten.push_block(Block::Opaque {
            kind: THEMATIC_BREAK_KIND.to_string(),
            payload_hint: String::new(),
            status: SemanticStatus::Resolved,
        });
        for (number, body) in &notes {
            rewritten.push_block(Block::Paragraph { content: vec![Inline::Text { text: format!("[{number}] {body}") }] });
        }
    }

    FootnoteLowering { graph: rewritten, diagnostics, references, definitions: definitions_count, notes: notes.len() }
}

/// Read one authored GFM definition payload into its label and plain body.
fn parse_definition(payload: &str) -> Option<(String, String)> {
    let rest = payload.trim().strip_prefix("[^")?;
    let (label, body) = rest.split_once("]:")?;
    let label = label.trim();
    let body = body.lines().map(str::trim).filter(|line| !line.is_empty()).collect::<Vec<_>>().join(" ");
    if label.is_empty() || body.is_empty() {
        return None;
    }
    Some((label.to_string(), body))
}

/// Visit every inline run of one block payload in document order.
fn visit_block_inlines(block: &Block, visit: &mut impl FnMut(&Inline)) {
    match block {
        Block::Section { title, .. } => visit_inlines(title, visit),
        Block::Paragraph { content } => visit_inlines(content, visit),
        Block::List { items, .. } => {
            for item in items {
                visit_inlines(&item.content, visit);
            }
        }
        Block::Table { rows, .. } => {
            for row in rows {
                for cell in &row.cells {
                    visit_inlines(&cell.content, visit);
                }
            }
        }
        Block::Quote { content, .. } => visit_inlines(content, visit),
        _ => {}
    }
}

/// Visit every inline run of one block payload for in-place rewriting.
fn for_each_block_inline(block: &mut Block, edit: &mut impl FnMut(&mut Inline)) {
    match block {
        Block::Section { title, .. } => edit_inlines(title, edit),
        Block::Paragraph { content } => edit_inlines(content, edit),
        Block::List { items, .. } => {
            for item in items {
                edit_inlines(&mut item.content, edit);
            }
        }
        Block::Table { rows, .. } => {
            for row in rows {
                for cell in &mut row.cells {
                    edit_inlines(&mut cell.content, edit);
                }
            }
        }
        Block::Quote { content, .. } => edit_inlines(content, edit),
        _ => {}
    }
}

/// Visit inline content recursively, children first-class.
fn visit_inlines(content: &[Inline], visit: &mut impl FnMut(&Inline)) {
    for inline in content {
        visit(inline);
        match inline {
            Inline::Styled { children, .. } | Inline::Attributed { children, .. } => visit_inlines(children, visit),
            Inline::Link { display, .. } => visit_inlines(display, visit),
            Inline::Image { alt, .. } => visit_inlines(alt, visit),
            Inline::Note { display: Some(display), .. } | Inline::Citation { display: Some(display), .. } => {
                visit_inlines(display, visit);
            }
            _ => {}
        }
    }
}

/// Rewrite inline content recursively, calling the editor on every run.
fn edit_inlines(content: &mut [Inline], edit: &mut impl FnMut(&mut Inline)) {
    for inline in content {
        edit(inline);
        match inline {
            Inline::Styled { children, .. } | Inline::Attributed { children, .. } => edit_inlines(children, edit),
            Inline::Link { display, .. } => edit_inlines(display, edit),
            Inline::Image { alt, .. } => edit_inlines(alt, edit),
            Inline::Note { display: Some(display), .. } | Inline::Citation { display: Some(display), .. } => {
                edit_inlines(display, edit);
            }
            _ => {}
        }
    }
}

/// Build one footnote diagnostic record, borrowing the block span when there is one.
fn record(
    code: &str,
    severity: &str,
    message: String,
    node: Option<NodeId>,
    graph: &DocumentGraph,
    path: &str,
) -> DiagnosticRecord {
    DiagnosticRecord {
        code: code.to_string(),
        severity: severity.to_string(),
        message,
        stage: STAGE.to_string(),
        path: Some(path.to_string()),
        span: node
            .and_then(|id| graph.block_span(id))
            .map(|span| DiagnosticSpan { start: span.start as usize, end: span.end as usize }),
    }
}
