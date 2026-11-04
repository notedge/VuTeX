//! Conversion report projection and fail-closed diagnostics for Typst imports.
//!
//! The adapter never re-parses Typst. It maps the loss records and opaque blocks
//! the Notedown lowering produced into the shared [`ConversionReport`], so a
//! construct that VuTeX cannot consume is visible to the host instead of
//! disappearing from the rendered page.

use notedown_ir::{Block, DocumentGraph, Inline, MathDisplay, MathExpression, MathFormula, SemanticStatus};
use vutex_types::{
    ConversionReport, CoverageSummary, Diagnostic, LossyItem, OpaqueItem, PreservedItem, Severity, SourceSpan, UnsupportedItem,
};

/// Diagnostic code returned when the Typst frontend cannot produce a document graph.
pub const IMPORT_FAILED: &str = "vutex.input.typst.import_failed";

/// Report code for math kept as source because no semantic lowering exists.
pub const MATH_SOURCE_PRESERVED: &str = "import.typst.math.source_preserved";

/// Diagnostic code for a dynamic construct that continues past its first line.
pub const UNTERMINATED_GROUP: &str = "vutex.input.typst.unterminated_dynamic_group";

/// Loss codes that summarize the whole import instead of one source construct.
const SUMMARY_LOSS_CODES: [&str; 2] = ["import.typst.partial_coverage", "import.typst.empty_document"];

/// Project the imported graph into the shared conversion report.
pub(crate) fn build_report(graph: &DocumentGraph) -> ConversionReport {
    let blocks = graph.blocks.len();
    let construct_losses = graph.coverage.loss.iter().filter(|marker| !is_summary(&marker.code)).count();
    let lowered_handled = graph.blocks.iter().filter(|node| is_lowered(&node.block)).count();

    let mut unsupported = Vec::new();
    let mut lossy = Vec::new();
    for marker in &graph.coverage.loss {
        match marker.status {
            SemanticStatus::Unsupported | SemanticStatus::Unresolved => {
                unsupported.push(UnsupportedItem { code: marker.code.clone(), detail: marker.message.clone() });
            }
            SemanticStatus::Partial | SemanticStatus::Lossy | SemanticStatus::Inferred => {
                lossy.push(LossyItem { code: marker.code.clone(), detail: marker.message.clone() });
            }
            SemanticStatus::Resolved => {}
        }
    }

    let opaque = graph
        .blocks
        .iter()
        .filter_map(|node| match &node.block {
            Block::Opaque { kind, payload_hint, .. } => Some(OpaqueItem { code: kind.clone(), detail: payload_hint.clone() }),
            _ => None,
        })
        .collect();

    ConversionReport {
        parsed: CoverageSummary { considered: count(blocks + construct_losses), handled: count(blocks) },
        lowered: CoverageSummary { considered: count(blocks), handled: count(lowered_handled) },
        preserved: collect_preserved_math(graph),
        unsupported,
        opaque,
        lossy,
    }
}

/// Build one diagnostic per construct-level loss plus a fallback per opaque
/// block the lowering never reported.
///
/// The lowering records loss without source spans, so spans are recovered from
/// the payload text in source order. Document-level summaries stay unpositioned.
pub(crate) fn build_diagnostics(graph: &DocumentGraph, source: &str) -> Vec<Diagnostic> {
    let mut locator = Locator::new(source);
    let mut diagnostics = Vec::new();
    let mut located_payloads = Vec::new();

    for marker in &graph.coverage.loss {
        if is_summary(&marker.code) {
            continue;
        }
        let payload = payload_after_colon(&marker.message).filter(|text| !text.is_empty());
        let span = payload.and_then(|text| locator.locate(text));
        if span.is_some() {
            if let Some(text) = payload {
                located_payloads.push(text.to_string());
            }
        }
        diagnostics.push(Diagnostic {
            code: marker.code.clone(),
            severity: severity(marker.status),
            message: marker.message.clone(),
            span,
        });
    }

    let mut opaque_locator = Locator::new(source);
    for node in &graph.blocks {
        let Block::Opaque { kind, payload_hint, status } = &node.block
        else {
            continue;
        };
        let payload = payload_hint.trim();
        if located_payloads.contains(&payload.to_string()) {
            continue;
        }
        diagnostics.push(Diagnostic {
            code: kind.clone(),
            severity: severity(*status),
            message: format!("Typst construct kept opaque without a lowering loss record: {payload}"),
            span: opaque_locator.locate(payload),
        });
    }

    diagnostics
}

fn is_summary(code: &str) -> bool {
    SUMMARY_LOSS_CODES.contains(&code)
}

/// Find a dynamic construct whose group continues past the line the lowering
/// kept opaque.
///
/// The current Notedown lowering retains only the first line of a dynamic
/// expression, so the rest of a multi-line group would be lowered as page text.
/// That is an approximation this adapter refuses instead of rendering.
pub(crate) fn unterminated_group(graph: &DocumentGraph, source: &str) -> Option<Diagnostic> {
    let mut locator = Locator::new(source);
    for node in &graph.blocks {
        let Block::Opaque { payload_hint, .. } = &node.block
        else {
            continue;
        };
        let payload = payload_hint.trim();
        if group_depth(payload) <= 0 {
            continue;
        }
        let span = locator.locate(payload);
        return Some(Diagnostic {
            code: UNTERMINATED_GROUP.to_string(),
            severity: Severity::Error,
            message: format!(
                "Typst dynamic construct opens a group the current lowering does not follow, its body would be lowered as page text: {payload}"
            ),
            span,
        });
    }
    None
}

/// Delimiter depth left open at the end of one opaque payload.
///
/// Quoted strings and line comments do not contribute, so `#text("(")` stays
/// balanced instead of looking like an open group.
fn group_depth(text: &str) -> i32 {
    let mut depth = 0i32;
    let mut in_string = false;
    let mut chars = text.chars().peekable();
    while let Some(character) = chars.next() {
        if in_string {
            match character {
                '\\' => {
                    chars.next();
                }
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match character {
            '"' => in_string = true,
            '/' if chars.peek() == Some(&'/') => break,
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            _ => {}
        }
    }
    depth
}

fn is_lowered(block: &Block) -> bool {
    match block {
        Block::Opaque { .. } => false,
        Block::Math { formula } => !matches!(formula.expression, MathExpression::Unsupported { .. }),
        _ => true,
    }
}

fn severity(status: SemanticStatus) -> Severity {
    match status {
        SemanticStatus::Unsupported | SemanticStatus::Unresolved => Severity::Error,
        SemanticStatus::Partial | SemanticStatus::Lossy | SemanticStatus::Inferred => Severity::Warning,
        SemanticStatus::Resolved => Severity::Info,
    }
}

/// Text after the first colon that introduces the retained payload.
///
/// The payload itself may contain colons, as in `#set page(width: 10cm)`, so the
/// split must take the first separator rather than the last one.
fn payload_after_colon(message: &str) -> Option<&str> {
    message.split_once(": ").map(|(_, payload)| payload.trim())
}

fn count(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

fn collect_preserved_math(graph: &DocumentGraph) -> Vec<PreservedItem> {
    let mut items = Vec::new();
    for node in &graph.blocks {
        match &node.block {
            Block::Section { title, .. } => collect_inline_math(title, &mut items),
            Block::Paragraph { content } => collect_inline_math(content, &mut items),
            Block::List { items: list_items, .. } => {
                for item in list_items {
                    collect_inline_math(&item.content, &mut items);
                }
            }
            Block::Table { rows, .. } => {
                for row in rows {
                    for cell in &row.cells {
                        collect_inline_math(&cell.content, &mut items);
                    }
                }
            }
            Block::Quote { content, .. } => collect_inline_math(content, &mut items),
            Block::Math { formula } => preserve_math(formula, &mut items),
            _ => {}
        }
    }
    items
}

fn collect_inline_math(inlines: &[Inline], items: &mut Vec<PreservedItem>) {
    for inline in inlines {
        match inline {
            Inline::InlineMath { formula } => preserve_math(formula, items),
            Inline::Styled { children, .. } | Inline::Attributed { children, .. } => {
                collect_inline_math(children, items);
            }
            Inline::Link { display, .. } => collect_inline_math(display, items),
            Inline::Image { alt, .. } => collect_inline_math(alt, items),
            Inline::Note { display: Some(display), .. } | Inline::Citation { display: Some(display), .. } => {
                collect_inline_math(display, items);
            }
            _ => {}
        }
    }
}

fn preserve_math(formula: &MathFormula, items: &mut Vec<PreservedItem>) {
    if !matches!(formula.expression, MathExpression::Unsupported { .. }) {
        return;
    }
    let display = match formula.display {
        MathDisplay::Inline => "inline",
        MathDisplay::Display => "display",
    };
    items.push(PreservedItem {
        code: MATH_SOURCE_PRESERVED.to_string(),
        detail: format!("{display} math kept as source text: {}", formula.source),
    });
}

/// Byte offsets recovered by searching the source in payload order.
struct Locator<'a> {
    source: &'a str,
    cursor: usize,
}

impl<'a> Locator<'a> {
    fn new(source: &'a str) -> Self {
        Self { source, cursor: 0 }
    }

    fn locate(&mut self, payload: &str) -> Option<SourceSpan> {
        if payload.is_empty() {
            return None;
        }
        let (start, length) = match self.source.get(self.cursor..).and_then(|tail| tail.find(payload)) {
            Some(offset) => (self.cursor + offset, payload.len()),
            None => (self.source.find(payload)?, payload.len()),
        };
        self.cursor = start + length;
        Some(SourceSpan { start: u32::try_from(start).ok()?, end: u32::try_from(start + length).ok()? })
    }
}

#[cfg(test)]
mod tests {
    use notedown_ir::{Block, DocumentGraph, DocumentId, SemanticStatus};

    use super::*;

    #[test]
    fn opaque_without_loss_record_still_reports_a_diagnostic() {
        let mut graph = DocumentGraph::new(DocumentId(1));
        graph.push_block(Block::Opaque {
            kind: "typst.expression.custom".to_string(),
            payload_hint: "#custom()".to_string(),
            status: SemanticStatus::Unsupported,
        });
        graph.push_block(Block::Paragraph { content: Vec::new() });

        let diagnostics = build_diagnostics(&graph, "#custom()\n\nBody text.\n");
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "typst.expression.custom");
        assert_eq!(diagnostics[0].severity, Severity::Error);
        assert_eq!(diagnostics[0].span, Some(SourceSpan { start: 0, end: 9 }));

        let report = build_report(&graph);
        assert_eq!(report.parsed.considered, 2);
        assert_eq!(report.parsed.handled, 2);
        assert_eq!(report.lowered.considered, 2);
        assert_eq!(report.lowered.handled, 1);
        assert_eq!(
            report.opaque,
            vec![OpaqueItem { code: "typst.expression.custom".to_string(), detail: "#custom()".to_string() }]
        );
    }

    #[test]
    fn summary_loss_codes_do_not_inflate_parsed_counts() {
        let mut graph = DocumentGraph::new(DocumentId(1));
        graph.push_block(Block::Paragraph { content: Vec::new() });
        graph.push_loss(notedown_ir::LossMarker {
            code: "import.typst.partial_coverage".to_string(),
            message: "two constructs had no exact mapping".to_string(),
            status: SemanticStatus::Partial,
        });

        let report = build_report(&graph);
        assert_eq!(report.parsed.considered, 1);
        assert!(report.lossy.iter().any(|item| item.code == "import.typst.partial_coverage"));
        assert!(build_diagnostics(&graph, "Body.\n").is_empty());
    }
}
