//! End-to-end checks for the Typst input adapter.

use vutex_input_typst::{MATH_SOURCE_PRESERVED, UNTERMINATED_GROUP, import_source, render_svg};
use vutex_types::{Severity, SourceSpan};

const MINIMAL: &str =
    "= Introduction\n\nHello *world* and _emphasis_ with a [link](https://example.com).\n\n- First item\n- Second item\n";

const SUBSET: &str = "= Top\n\n== Second level\n\n1. one\n2. two\n\n- outer\n  - inner\n\n```rust\nfn main() {}\n```\n\nA [named link](https://example.com \"title\") and *bold* text.\n";

const DYNAMIC: &str = "= Report\n\n#set page(width: 10cm)\n\nBody text.\n";

const MULTILINE_FIGURE: &str = "= Figure\n\n#figure(\n  rect(width: 2cm),\n  caption: [A box],\n)\n\nTail text.\n";

const MATH: &str = "= Formula\n\nThe value $x + y$ matters.\n\n$ a = b $\n";

#[test]
fn minimal_subset_renders_svg_through_the_layout_chain() {
    let rendered = render_svg("minimal.typ", MINIMAL).expect("minimal Typst subset imports");

    assert!(rendered.svg.contains("<svg"));
    assert!(rendered.svg.contains("Introduction"));
    assert!(rendered.svg.contains("world"));
    assert!(rendered.svg.contains("First item"));
    assert_eq!(rendered.pages, 1);
    assert!(rendered.items > 0);

    assert_eq!(rendered.report.lowered.considered, rendered.report.lowered.handled);
    assert!(rendered.report.preserved.is_empty());
    assert!(rendered.report.unsupported.is_empty());
    assert!(rendered.report.opaque.is_empty());

    let json = serde_json::to_value(&rendered.report).expect("report serializes");
    assert!(json.get("complete").is_none(), "report has no top-level complete flag: {json}");
}

#[test]
fn dynamic_construct_is_opaque_with_an_exact_source_span() {
    let rendered = render_svg("dynamic.typ", DYNAMIC).expect("dynamic Typst imports");

    assert!(rendered.svg.contains("Report"));
    assert!(rendered.svg.contains("Body text."));
    assert!(!rendered.svg.contains("10cm"), "opaque construct must not reach the page: {}", rendered.svg);

    assert_eq!(rendered.report.parsed.considered, 4);
    assert_eq!(rendered.report.parsed.handled, 3);
    assert_eq!(rendered.report.lowered.considered, 3);
    assert_eq!(rendered.report.lowered.handled, 2);

    let unsupported: Vec<&str> = rendered.report.unsupported.iter().map(|item| item.code.as_str()).collect();
    assert_eq!(unsupported, vec!["import.typst.dynamic_content"]);

    assert_eq!(rendered.report.opaque.len(), 1);
    assert_eq!(rendered.report.opaque[0].code, "typst.expression.set");
    assert_eq!(rendered.report.opaque[0].detail, "#set page(width: 10cm)");

    let start = DYNAMIC.find("#set page(width: 10cm)").expect("construct exists in source");
    assert_eq!(rendered.diagnostics.len(), 1, "one construct-level diagnostic: {:?}", rendered.diagnostics);
    let diagnostic = &rendered.diagnostics[0];
    assert_eq!(diagnostic.code, "import.typst.dynamic_content");
    assert_eq!(diagnostic.severity, Severity::Error);
    assert_eq!(diagnostic.span, Some(SourceSpan { start: start as u32, end: (start + "#set page(width: 10cm)".len()) as u32 }));
}

#[test]
fn typst_math_keeps_its_source_and_reports_the_layout_gap() {
    let document = import_source("math.typ", MATH).expect("Typst math imports");

    assert_eq!(document.report.preserved.len(), 2, "inline and display math: {:?}", document.report.preserved);
    assert!(document.report.preserved.iter().all(|item| item.code == MATH_SOURCE_PRESERVED));

    let warnings: Vec<&str> = document
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "vutex.layout.math.unsupported_language")
        .map(|diagnostic| diagnostic.code.as_str())
        .collect();
    assert!(warnings.is_empty(), "layout diagnostics belong to the render stage: {:?}", document.diagnostics);

    let rendered = render_svg("math.typ", MATH).expect("Typst math renders as text");
    assert!(rendered.svg.contains("x + y"), "inline math source stays visible: {}", rendered.svg);
    assert!(rendered.svg.contains("a = b"), "display math source stays visible: {}", rendered.svg);
    assert_eq!(rendered.svg.matches("vutex.layout.math.unsupported_language").count(), 0);

    let layout_warnings: Vec<_> =
        rendered.diagnostics.iter().filter(|diagnostic| diagnostic.code == "vutex.layout.math.unsupported_language").collect();
    assert_eq!(layout_warnings.len(), 2, "one warning per formula: {:?}", rendered.diagnostics);
    assert!(layout_warnings.iter().all(|diagnostic| diagnostic.severity == Severity::Warning));
}

#[test]
fn whitespace_only_source_reports_the_document_level_loss() {
    let document = import_source("empty.typ", "   \n\n").expect("empty source still imports");

    assert!(document.graph.blocks.is_empty());
    assert_eq!(document.report.parsed.considered, 0);
    assert_eq!(document.report.lowered.handled, 0);
    assert!(document.report.lossy.iter().any(|item| item.code == "import.typst.empty_document"));
    assert!(document.diagnostics.is_empty(), "summary loss stays unpositioned: {:?}", document.diagnostics);
}

#[test]
fn documented_subset_lowers_without_loss() {
    let rendered = render_svg("subset.typ", SUBSET).expect("documented subset imports");

    for probe in ["Top", "Second level", "one", "inner", "fn main", "named link"] {
        assert!(rendered.svg.contains(probe), "expected `{probe}` in the page: {}", rendered.svg);
    }

    assert_eq!(rendered.report.parsed.considered, 7);
    assert_eq!(rendered.report.parsed.handled, 7);
    assert_eq!(rendered.report.lowered.considered, 7);
    assert_eq!(rendered.report.lowered.handled, 7);
    assert!(rendered.report.unsupported.is_empty());
    assert!(rendered.report.opaque.is_empty());
    assert!(rendered.diagnostics.is_empty(), "no construct-level diagnostics: {:?}", rendered.diagnostics);
}

#[test]
fn multi_line_dynamic_group_fails_closed() {
    let error = import_source("figure.typ", MULTILINE_FIGURE).expect_err("the construct body would leak as text");

    assert_eq!(error.code(), UNTERMINATED_GROUP);
    assert_eq!(error.diagnostic.severity, Severity::Error);
    let start = MULTILINE_FIGURE.find("#figure(").expect("construct exists in source");
    assert_eq!(error.diagnostic.span, Some(SourceSpan { start: start as u32, end: (start + "#figure(".len()) as u32 }));

    let rendered = render_svg("figure.typ", MULTILINE_FIGURE);
    assert!(rendered.is_err(), "no approximate page may be produced");
    assert_eq!(rendered.expect_err("refused").code(), UNTERMINATED_GROUP);
}

#[test]
fn single_line_dynamic_group_with_balanced_strings_still_imports() {
    let source = "= Report\n\n#text(\"(\")\n\nBody text.\n";
    let document = import_source("balanced.typ", source).expect("balanced dynamic group imports");

    assert_eq!(document.report.unsupported.len(), 1);
    assert_eq!(document.report.unsupported[0].code, "import.typst.dynamic_content");
}
