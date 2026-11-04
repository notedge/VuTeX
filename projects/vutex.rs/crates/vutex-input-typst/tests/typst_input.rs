//! End-to-end checks for the Typst input adapter.

use vutex_input_typst::{MATH_SOURCE_PRESERVED, UNTERMINATED_GROUP, import_source, render_svg};
use vutex_types::{Severity, SourceSpan};

const MINIMAL: &str =
    "= Introduction\n\nHello *world* and _emphasis_ with a [link](https://example.com).\n\n- First item\n- Second item\n";

const SUBSET: &str = "= Top\n\n== Second level\n\n1. one\n2. two\n\n- outer\n  - inner\n\n```rust\nfn main() {}\n```\n\nA [named link](https://example.com \"title\") and *bold* text.\n";

const DYNAMIC: &str = "= Report\n\n#set page(width: 10cm)\n\nBody text.\n";

const MULTILINE_FIGURE: &str = "= Figure\n\n#figure(\n  rect(width: 2cm),\n  caption: [A box],\n)\n\nTail text.\n";

const MATH: &str = "= Formula\n\nThe value $x + y$ matters.\n\n$ a = b $\n";

const COMMENTS: &str = "= Title\n\n// line comment\n\nText // trailing\n\n/* block\n   comment */\n\nTail.\n";

const ESCAPES: &str = "= Title\n\nA \\#hash and \\*star\\* and \\$dollar\\$ and \\_under\\_.\n";

const MARKUP: &str = "= Title\n\nSome `code` and a~space here.\n";

const TABLE: &str = "= Title\n\n#table(columns: 2, [a], [b])\n\nTail.\n";

const PLACE: &str = "= Title\n\n#place(top + right, [note])\n\nBody.\n";

const FLOAT: &str = "= Title\n\n#float(caption: [Chart], rect())\n\nBody.\n";

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

#[test]
fn comments_produce_no_text_nodes() {
    let rendered = render_svg("comments.typ", COMMENTS).expect("comments import");

    assert!(rendered.svg.contains("Title"));
    assert!(rendered.svg.contains("Text"));
    assert!(rendered.svg.contains("Tail."));
    for leaked in ["line comment", "trailing", "block", "comment */"] {
        assert!(!rendered.svg.contains(leaked), "comment text leaked into the page: {leaked}");
    }

    assert_eq!(rendered.report.parsed.considered, 2);
    assert_eq!(rendered.report.parsed.handled, 2);
    assert_eq!(rendered.report.lowered.handled, 2);
    assert!(rendered.report.unsupported.is_empty());
    assert!(rendered.report.opaque.is_empty());
    assert!(rendered.diagnostics.is_empty(), "comments carry no diagnostics: {:?}", rendered.diagnostics);
}

#[test]
fn inline_markup_keeps_structured_nodes() {
    let document = import_source("markup.typ", MARKUP).expect("inline markup imports");

    let paragraph = document
        .graph
        .blocks
        .iter()
        .find_map(|node| match &node.block {
            notedown_ir::Block::Paragraph { content } => Some(content),
            _ => None,
        })
        .expect("markup paragraph exists");

    assert!(
        paragraph.iter().any(|inline| matches!(inline, notedown_ir::Inline::InlineCode { text } if text == "code")),
        "inline code stays a structured node: {paragraph:?}"
    );
    assert!(
        paragraph.iter().any(|inline| matches!(inline, notedown_ir::Inline::Text { text } if text.contains("space"))),
        "surrounding text stays a text node: {paragraph:?}"
    );

    let rendered = render_svg("markup.typ", MARKUP).expect("inline markup renders");
    assert!(rendered.svg.contains("code"));
    assert!(rendered.svg.contains("space"));
    assert!(rendered.report.unsupported.is_empty());
    assert!(rendered.report.opaque.is_empty());
    assert!(rendered.diagnostics.is_empty(), "markup carries no diagnostics: {:?}", rendered.diagnostics);
}

#[test]
fn escaped_markup_is_reported_and_never_rendered_as_text() {
    let document = import_source("escapes.typ", ESCAPES).expect("escaped markup imports");

    assert_eq!(document.report.unsupported.len(), 1);
    assert_eq!(document.report.unsupported[0].code, "import.typst.dynamic_content");
    assert_eq!(document.report.opaque.len(), 1);
    assert_eq!(document.report.opaque[0].code, "typst.expression");
    assert_eq!(document.diagnostics.len(), 1);
    assert_eq!(document.diagnostics[0].severity, Severity::Error);
    assert_eq!(document.diagnostics[0].span, Some(SourceSpan { start: 11, end: 53 }));

    let rendered = render_svg("escapes.typ", ESCAPES).expect("escaped markup renders without the escapes");
    for leaked in ["hash", "star", "dollar", "under"] {
        assert!(!rendered.svg.contains(leaked), "escaped source leaked into the page: {leaked}");
    }
}

#[test]
fn single_line_table_is_one_opaque_item() {
    let rendered = render_svg("table.typ", TABLE).expect("single-line table imports");

    assert_eq!(rendered.report.parsed.considered, 4);
    assert_eq!(rendered.report.parsed.handled, 3);
    assert_eq!(rendered.report.lowered.considered, 3);
    assert_eq!(rendered.report.lowered.handled, 2);
    assert_eq!(rendered.report.opaque.len(), 1);
    assert_eq!(rendered.report.opaque[0].code, "typst.expression.table");
    assert_eq!(rendered.report.opaque[0].detail, "#table(columns: 2, [a], [b])");
    assert_eq!(rendered.report.unsupported.len(), 1);
    assert_eq!(rendered.report.unsupported[0].code, "import.typst.dynamic_content");
    assert_eq!(rendered.diagnostics.len(), 1);
    assert_eq!(rendered.diagnostics[0].span, Some(SourceSpan { start: 9, end: 37 }));

    assert!(rendered.svg.contains("Tail."));
    assert!(!rendered.svg.contains("columns"), "table payload must not reach the page");
}

#[test]
fn single_line_layout_intent_stays_opaque() {
    for (label, source, code, payload, visible, hidden) in [
        ("place.typ", PLACE, "typst.expression.place", "#place(top + right, [note])", "Body.", "note"),
        ("float.typ", FLOAT, "typst.expression.float", "#float(caption: [Chart], rect())", "Body.", "Chart"),
    ] {
        let rendered = render_svg(label, source).expect("single-line layout intent imports");

        assert!(rendered.svg.contains(visible), "{label}: surrounding text stays on the page");
        assert!(!rendered.svg.contains(hidden), "{label}: layout intent must not reach the page: {}", rendered.svg);

        assert_eq!(rendered.report.parsed.considered, 4, "{label}");
        assert_eq!(rendered.report.parsed.handled, 3, "{label}");
        assert_eq!(rendered.report.lowered.considered, 3, "{label}");
        assert_eq!(rendered.report.lowered.handled, 2, "{label}");

        assert_eq!(rendered.report.opaque.len(), 1, "{label}: {:?}", rendered.report.opaque);
        assert_eq!(rendered.report.opaque[0].code, code, "{label}");
        assert_eq!(rendered.report.opaque[0].detail, payload, "{label}");

        let unsupported: Vec<&str> = rendered.report.unsupported.iter().map(|item| item.code.as_str()).collect();
        assert_eq!(unsupported, vec!["import.typst.dynamic_content"], "{label}");

        assert_eq!(rendered.diagnostics.len(), 1, "{label}: {:?}", rendered.diagnostics);
        let diagnostic = &rendered.diagnostics[0];
        assert_eq!(diagnostic.code, "import.typst.dynamic_content", "{label}");
        assert_eq!(diagnostic.severity, Severity::Error, "{label}");
        let start = source.find(payload).expect("layout intent exists in source");
        assert_eq!(diagnostic.span, Some(SourceSpan { start: start as u32, end: (start + payload.len()) as u32 }), "{label}");
    }
}

#[test]
fn every_opaque_item_carries_a_positioned_diagnostic() {
    for (label, source) in [
        ("minimal.typ", MINIMAL),
        ("subset.typ", SUBSET),
        ("dynamic.typ", DYNAMIC),
        ("table.typ", TABLE),
        ("place.typ", PLACE),
        ("float.typ", FLOAT),
        ("escapes.typ", ESCAPES),
    ] {
        let document = import_source(label, source).expect("document imports");

        for item in &document.report.opaque {
            let matched = document
                .diagnostics
                .iter()
                .find(|diagnostic| diagnostic.code == item.code || diagnostic.message.contains(item.detail.trim()));
            let diagnostic = matched
                .unwrap_or_else(|| panic!("{label}: opaque `{}` has no diagnostic: {:?}", item.code, document.diagnostics));
            assert!(diagnostic.span.is_some(), "{label}: diagnostic for `{}` is unpositioned", item.code);
        }
    }
}
