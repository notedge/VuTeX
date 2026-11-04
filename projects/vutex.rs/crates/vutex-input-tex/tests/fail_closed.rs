//! Fail-closed contract: rejected input, opaque constructs, and foreign math
//! language tags produce structured diagnostics instead of silent degradation.

use notedown_ir::{Block, DocumentGraph, DocumentId, Inline, ListItem, MathDisplay, MathFormula};
use vutex_input_tex::{
    INVALID_GRAPH, MATH_LANGUAGE, PARSE_FAILED, conversion_report, import_file, import_source, validate_graph,
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

    assert_eq!(error.code(), PARSE_FAILED);
    assert!(!error.message().is_empty(), "the frontend message must be preserved");
    assert_eq!(error.diagnostic.severity, Severity::Error);
}

#[test]
fn unknown_environment_is_reported_with_its_source_span() {
    let document = import_source("figure.tex", UNKNOWN_ENVIRONMENT).expect("the frontend accepts the syntax");

    assert_eq!(document.report.opaque.len(), 1, "one environment must stay opaque: {:#?}", document.report);
    assert_eq!(document.report.opaque[0].code, "tex.environment.figure");
    assert!(
        document.report.unsupported.iter().any(|item| item.code == "import.tex.unknown_environment"),
        "the lowering loss must reach the report: {:#?}",
        document.report.unsupported
    );

    let opaque = document
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "tex.environment.figure")
        .expect("the opaque environment must be diagnosed");
    let span = opaque.span.expect("the authored environment must be located in the source");
    assert_eq!(&UNKNOWN_ENVIRONMENT[span.start as usize..span.end as usize], document.report.opaque[0].detail);
    assert_eq!(opaque.severity, Severity::Warning, "opaque content is a reported loss, not a failed import");
    assert!(
        !document.diagnostics.iter().any(|diagnostic| diagnostic.code == MATH_LANGUAGE),
        "opaque content is not a math language failure"
    );
}

#[test]
fn invalid_graph_is_rejected_before_layout() {
    let mut graph = DocumentGraph::new(DocumentId(9));
    graph.push_block(Block::List {
        ordered: true,
        items: vec![ListItem {
            content: vec![Inline::Text { text: "no number".to_string() }],
            children: Vec::new(),
            number: None,
            is_task: false,
            is_checked: None,
        }],
    });

    let error = validate_graph(&graph).expect_err("an ordered item without a number breaks the list invariant");
    assert_eq!(error.code(), INVALID_GRAPH);
    assert!(error.message().contains("MissingListItemNumber"), "the invariant must be named: {}", error.message());
}

#[test]
fn foreign_math_language_fails_closed() {
    let mut graph = DocumentGraph::new(DocumentId(7));
    graph.push_block(Block::Math {
        formula: MathFormula::unsupported("x^2", Some("typst".to_string()), MathDisplay::Display, "typst"),
    });

    let report = conversion_report(&graph);
    assert!(
        report.unsupported.iter().any(|item| item.code == MATH_LANGUAGE),
        "foreign math must be reported: {:#?}",
        report.unsupported
    );
    assert_eq!(report.lowered.handled, 0, "an unsupported formula is not a lowered block");

    let diagnostics = vutex_input_tex::conversion_diagnostics(&graph, "");
    let mismatch = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == MATH_LANGUAGE)
        .expect("foreign math must be diagnosed");
    assert_eq!(mismatch.severity, Severity::Error, "a foreign formula must not pass silently to TeX layout");
}

#[test]
fn tex_math_without_a_language_tag_stays_accepted() {
    let mut graph = DocumentGraph::new(DocumentId(8));
    graph.push_block(Block::Math {
        formula: MathFormula::unsupported("x^2", None, MathDisplay::Inline, "tex"),
    });

    let report = conversion_report(&graph);
    assert!(report.unsupported.is_empty(), "untagged math belongs to the TeX profile: {:#?}", report.unsupported);
    assert!(vutex_input_tex::conversion_diagnostics(&graph, "").is_empty());
}

#[test]
fn file_import_reads_the_same_source() {
    let path = std::env::temp_dir().join("vutex-input-tex-file.tex");
    std::fs::write(&path, "Text $x^2$ in a file.\n").expect("the fixture must be written");
    let document = import_file(&path).expect("the file must import");
    assert_eq!(document.report.parsed.handled, 1);
    std::fs::remove_file(&path).expect("the fixture must be removed");
}
