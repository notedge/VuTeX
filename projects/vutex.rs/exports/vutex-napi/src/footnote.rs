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
    /// Body paragraphs the definition carries in authored order.
    body: Vec<String>,
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
        for (number, paragraphs) in &notes {
            for (index, paragraph) in paragraphs.iter().enumerate() {
                let text = if index == 0 { format!("[{number}] {paragraph}") } else { paragraph.clone() };
                rewritten.push_block(Block::Paragraph { content: vec![Inline::Text { text }] });
            }
        }
    }

    FootnoteLowering { graph: rewritten, diagnostics, references, definitions: definitions_count, notes: notes.len() }
}

/// Read one authored GFM definition payload into its label and body paragraphs.
fn parse_definition(payload: &str) -> Option<(String, Vec<String>)> {
    let rest = payload.trim().strip_prefix("[^")?;
    let (label, body) = rest.split_once("]:")?;
    let label = label.trim();
    let paragraphs = split_paragraphs(body);
    if label.is_empty() || paragraphs.is_empty() {
        return None;
    }
    Some((label.to_string(), paragraphs))
}

/// Split a definition body on blank lines and flatten each paragraph to one line.
fn split_paragraphs(body: &str) -> Vec<String> {
    let mut paragraphs = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() {
            if !current.is_empty() {
                paragraphs.push(current.join(" "));
                current.clear();
            }
        }
        else {
            current.push(line);
        }
    }
    if !current.is_empty() {
        paragraphs.push(current.join(" "));
    }
    paragraphs
}

/// Absorb the GFM continuation paragraphs that the Markdown importer split off
/// as indented code blocks.
///
/// GFM keeps a definition open across blank lines while the following paragraphs
/// stay indented by four spaces or one tab. `oak-markdown` ends the definition
/// at its first line and lowers the indented paragraph as `Block::Code`, so the
/// body would move into the document text. This scans the authored source from
/// the definition end, appends each continuation paragraph to the payload, and
/// deletes the blocks the importer placed for them.
pub fn absorb_definition_continuations(graph: &mut DocumentGraph, source: &str) -> usize {
    let mut paragraphs_absorbed = 0;
    let mut index = 0;
    while index < graph.blocks.len() {
        let node_id = graph.blocks[index].id;
        let is_definition = matches!(&graph.blocks[index].block, Block::Opaque { kind, .. } if kind == DEFINITION_KIND);
        let Some(span) = graph.block_span(node_id)
        else {
            index += 1;
            continue;
        };
        if !is_definition {
            index += 1;
            continue;
        }
        let (paragraphs, _) = scan_continuations(source, span.end as usize);
        if paragraphs.is_empty() {
            index += 1;
            continue;
        }
        if let Block::Opaque { payload_hint, .. } = &mut graph.blocks[index].block {
            for paragraph in &paragraphs {
                payload_hint.push_str("\n\n");
                payload_hint.push_str(paragraph);
            }
        }
        // The importer turns each indented continuation paragraph into a
        // `Block::Code` without a span, so match them by content in order.
        let mut absorbed = Vec::new();
        let mut probe = index + 1;
        for paragraph in &paragraphs {
            let Some(node) = graph.blocks.get(probe)
            else {
                break;
            };
            let matches_paragraph = matches!(
                &node.block,
                Block::Code { language: None, content } if normalize(content) == normalize(paragraph)
            );
            if !matches_paragraph {
                break;
            }
            absorbed.push(node.id);
            probe += 1;
        }
        for id in absorbed {
            let _ = graph.delete_block(id);
        }
        paragraphs_absorbed += paragraphs.len();
        index += 1;
    }
    paragraphs_absorbed
}

/// Collapse whitespace so a code payload and its authored paragraph compare equal.
fn normalize(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Read blank-line-separated, four-space-indented paragraphs after a definition.
///
/// Returns the paragraphs without their indentation plus the byte offset where
/// the consumed continuation run ends.
fn scan_continuations(source: &str, from: usize) -> (Vec<String>, usize) {
    let mut paragraphs = Vec::new();
    let mut cursor = from;
    loop {
        let Some(blank_start) = consume_line_break(source, cursor)
        else {
            break;
        };
        let blank_end = line_end(source, blank_start);
        if !source[blank_start..blank_end].trim().is_empty() {
            break;
        }
        let Some(content_start) = consume_line_break(source, blank_end)
        else {
            break;
        };
        let rest = &source[content_start..];
        if !(rest.starts_with("    ") || rest.starts_with('\t')) {
            break;
        }
        let mut lines = Vec::new();
        let mut line_start = content_start;
        let mut paragraph_end = content_start;
        loop {
            let line_end = line_end(source, line_start);
            let line = &source[line_start..line_end];
            if line.trim().is_empty() || !(line.starts_with("    ") || line.starts_with('\t')) {
                break;
            }
            lines.push(strip_indent(line));
            paragraph_end = line_end;
            match consume_line_break(source, line_end) {
                Some(next) => line_start = next,
                None => break,
            }
        }
        if lines.is_empty() {
            break;
        }
        paragraphs.push(lines.join(" "));
        cursor = paragraph_end;
        if paragraphs.len() >= 64 {
            break;
        }
    }
    (paragraphs, cursor)
}

/// Consume one line break at `at` and return the next line start.
fn consume_line_break(source: &str, at: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    match bytes.get(at) {
        Some(b'\r') if bytes.get(at + 1) == Some(&b'\n') => Some(at + 2),
        Some(b'\r') | Some(b'\n') => Some(at + 1),
        _ => None,
    }
}

/// Byte offset of the first line break at or after `from`.
fn line_end(source: &str, from: usize) -> usize {
    source[from..].find(['\r', '\n']).map(|index| from + index).unwrap_or(source.len())
}

/// Remove the four-space or one-tab continuation indent from one line.
fn strip_indent(line: &str) -> &str {
    line.strip_prefix('\t').or_else(|| line.strip_prefix("    ")).unwrap_or(line)
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
        // Table cells and quotations now hold child block ids instead of
        // inline runs. Those child blocks are nodes of the same graph and are
        // visited when the flat block list reaches them.
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
        // Table cells and quotations now hold child block ids instead of
        // inline runs. Those child blocks are nodes of the same graph and are
        // edited when the flat block list reaches them.
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

#[cfg(test)]
mod tests {
    use super::{parse_definition, scan_continuations};

    #[test]
    fn a_single_line_definition_keeps_one_body_paragraph() {
        let (label, body) = parse_definition("[^1]: Only paragraph.").expect("definition parses");
        assert_eq!(label, "1");
        assert_eq!(body, vec!["Only paragraph."]);
    }

    #[test]
    fn two_continuation_paragraphs_stay_two_paragraphs() {
        let (_, body) = parse_definition("[^2]: First paragraph.\n\n    Second paragraph.").expect("definition parses");
        assert_eq!(body, vec!["First paragraph.", "Second paragraph."]);
    }

    #[test]
    fn three_continuation_paragraphs_stay_three_paragraphs() {
        let (_, body) = parse_definition("[^3]: One.\n\n    Two.\n\n    Three.").expect("definition parses");
        assert_eq!(body, vec!["One.", "Two.", "Three."]);
    }

    #[test]
    fn continuation_markup_survives_inside_its_paragraph() {
        let (_, body) =
            parse_definition("[^4]: Inline **bold**, `code`, and [link](https://example.test).\n\n    Second keeps *stress*.")
                .expect("definition parses");
        assert_eq!(body.len(), 2);
        assert!(body[0].contains("**bold**"), "{body:?}");
        assert!(body[0].contains("`code`"), "{body:?}");
        assert!(body[1].contains("*stress*"), "{body:?}");
    }

    #[test]
    fn the_continuation_scan_needs_one_blank_line_and_four_spaces() {
        let source = "[^1]: First.\n\n    Second.\n\n    Third.\nTail.\n";
        let (paragraphs, consumed_end) = scan_continuations(source, "[^1]: First.".len());
        assert_eq!(paragraphs, vec!["Second.", "Third."]);
        assert!(consumed_end < source.len());

        let inline = "[^1]: First.\n    Not a continuation.\n";
        let (paragraphs, _) = scan_continuations(inline, "[^1]: First.".len());
        assert!(paragraphs.is_empty(), "{paragraphs:?}");
    }
}
