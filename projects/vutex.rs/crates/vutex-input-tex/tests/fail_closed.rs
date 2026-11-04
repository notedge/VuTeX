//! Fail-closed contract: rejected input, opaque constructs, and foreign math
//! language tags produce structured diagnostics instead of silent degradation.

use notedown_ir::{Block, DocumentGraph, DocumentId, MathDisplay, MathFormula};
use vutex_input_tex::{
    ConversionStatus, MATH_LANGUAGE, OPAQUE_CONSTRUCT, PARSE_FAILED, conversion_report, import_source,
};
use vutex_types::Severity;

const UNKNOWN_ENVIRONMENT: &str = r"\begin{document}
\begin{figure}
\centering
\includegraphics{plot.pdf}
\end{figure}
\end{document}
";

#[test]
fn rejected_source_returns_a_structured_error() {
    let error = import_source("broken.tex", r"\begin{document}
\section{unterminated
\end{document}
")
    .expect_err("an unclosed group must not yield a partial document");

    assert_eq!(error.code, PARSE_FAILED);
    assert!(!error.message.is_empty(), "the frontend message must be preserved");
}

#[test]
fn unknown_environment_is_reported_with_its_source_span() {
    let document = import_source("figure.tex", UNKNOWN_ENVIRONMENT).expect("the frontend accepts the syntax");

    let opaque: Vec<_> = document
        .report
        .entries
        .iter()
        .filter(|entry| entry.status == ConversionStatus::Opaque)
        .collect();
    assert_eq!(opaque.len(), 1, "one environment must stay opaque: {:#?}", document.report.entries);
    let span = opaque[0].span.expect("the authored environment must be located in the source");
    let text = &UNKNOWN_ENVIRONMENT[span.start as usize..span.end as usize];
    assert_eq!(text, opaque[0].message.split_once(": ").map_or("", |(_, payload)| payload).trim());

    assert!(
        document.report.diagnostics.iter().any(|diagnostic| diagnostic.code == OPAQUE_CONSTRUCT
            && diagnostic.severity == Severity::Warning
            && diagnostic.span.is_some()),
        "opaque content must surface a warning carrying the span: {:#?}",
        document.report.diagnostics
    );
    assert!(
        !document.report.diagnostics.iter().any(|diagnostic| diagnostic.code == MATH_LANGUAGE),
        "opaque content is not a math language failure"
    );
    assert!(matches!(document.graph.blocks.iter().find(|node| matches!(node.block, Block::Opaque { .. })).map(|node| &node.block), Some(Block::Opaque { .. })));
}

#[test]
fn foreign_math_language_fails_closed() {
    let mut graph = DocumentGraph::new(DocumentId(7));
    graph.push_block(Block::Math {
        formula: MathFormula::unsupported("x^2", Some("typst".to_string()), MathDisplay::Display, "typst"),
    });

    let report = conversion_report(&graph, "");
    let entry = report
        .entries
        .iter()
        .find(|entry| entry.code == MATH_LANGUAGE)
        .expect("foreign math must be reported");
    assert_eq!(entry.status, ConversionStatus::Unsupported);
    assert!(report.has_errors(), "a foreign formula must not pass silently to TeX layout: {:#?}", report.diagnostics);
    assert_eq!(report.count(ConversionStatus::Parsed), 1, "the audit reports the accepted syntax once");
}

#[test]
fn tex_math_without_a_language_tag_stays_accepted() {
    let mut graph = DocumentGraph::new(DocumentId(8));
    graph.push_block(Block::Math {
        formula: MathFormula::unsupported("x^2", None, MathDisplay::Inline, "tex"),
    });

    let report = conversion_report(&graph, "");
    assert_eq!(report.count(ConversionStatus::Unsupported), 0, "untagged math belongs to the TeX profile: {:#?}", report.entries);
    assert!(!report.has_errors());
}
