//! Smoke lane for the document primitives the Node binding exposes.
//!
//! The binding is a thin wrapper: import, lowering, layout, presentation, and
//! rendering all live in the `document` family, which owns the per-lane
//! coverage tests. What can still break here is the wiring between that family
//! and this crate, so this lane proves one minimal source per input format
//! reaches an SVG payload through the same primitives the CLI composes, and
//! that the diagnostic and report primitives carry the expected data.
//!
//! Because the primitives no longer read paths or return exit codes, this lane
//! also pins the boundary: formats are selected by id, bytes are supplied by
//! the caller, and failures come back as diagnostic records.

use serde_json::Value;
use vutex_napi::document::{
    ImportFormat, TemplateColors, TemplateGeometry, assemble_report, build_display, build_flow,
    lower_document, parse_document, render_display_snapshot_html, render_display_svg,
    render_flow_html,
};
use vutex_layout::hint::{ChromeHeader, SectionChrome};
use vutex_napi::theme::Theme;
use vutex_napi::footnote::{
    FOOTNOTE_DEFINITION_DUPLICATE, FOOTNOTE_DEFINITION_MISSING, FOOTNOTE_DEFINITION_UNREFERENCED,
    FOOTNOTE_REFERENCE_REPEATED,
};

/// Parse, lower, and emit SVG for one source.
fn svg_of(name: &str, format: ImportFormat, source: &[u8]) -> (String, Value) {
    let parsed = parse_document(name, format, source).expect("source parses");
    let lowered = lower_document(parsed.graph, name);
    let footnotes = serde_json::to_value(lowered.footnotes).expect("counts serialize");
    let diagnostics = serde_json::to_value(&lowered.diagnostics).expect("diagnostics serialize");
    let scene = build_display(lowered, Theme::Baseline, None, None);
    let svg = render_display_svg(&scene).expect("svg renders");
    (svg, serde_json::json!({ "footnotes": footnotes, "diagnostics": diagnostics }))
}

#[test]
fn markdown_source_reaches_svg() {
    let (svg, _) = svg_of("sample.md", ImportFormat::Markdown, b"# Title\n\nHello world.\n");
    assert!(svg.contains("<svg"), "{svg}");
    assert!(svg.contains("@font-face"), "the SVG lane skipped font embedding");
}

#[test]
fn template_third_layer_reaches_layout_and_paint() {
    let parsed = parse_document(
        "sample.md",
        ImportFormat::Markdown,
        b"# One\n\nBody.\n\n## Sub\n\nMore.\n\n# Two\n\nEnd.\n",
    )
    .expect("source parses");
    let lowered = lower_document(parsed.graph, "sample.md");
    let template = TemplateGeometry {
        page_width: 612.0,
        page_height: 792.0,
        chrome_folio: Some(true),
        chrome_start_page: Some(1),
        chrome_sections: [
            Some(SectionChrome { header: Some(ChromeHeader::Literal("PART ONE")), folio: Some(false) }),
            Some(SectionChrome { header: Some(ChromeHeader::Literal("SUBSECTION")), folio: Some(true) }),
            None,
            None,
        ],
        section_breaks: [true, true, false, false],
        paragraph_gap: Some(12.0),
        paragraph_indent: Some(24.0),
        colors: TemplateColors { ink: Some("#112233"), ..Default::default() },
        ..Default::default()
    };
    let scene = build_display(lowered, Theme::Baseline, None, Some(template));
    let svg = render_display_svg(&scene).expect("svg renders");
    assert!(svg.contains("PART ONE"), "{svg}");
    assert!(svg.contains("SUBSECTION"), "{svg}");
    assert!(svg.contains("fill=\"#112233\""), "{svg}");
}

#[test]
fn tex_source_reaches_svg() {
    let (svg, _) = svg_of("sample.tex", ImportFormat::Tex, b"\\section{Introduction}\n\nHello world.\n");
    assert!(svg.contains("<svg"), "{svg}");
}

#[test]
fn typst_source_reaches_svg() {
    let (svg, _) = svg_of("sample.typ", ImportFormat::Typst, b"= Introduction\n\nHello world.\n");
    assert!(svg.contains("<svg"), "{svg}");
}

#[test]
fn tikz_fence_reaches_a_diagram_scene() {
    let (svg, _) = svg_of(
        "sample.md",
        ImportFormat::Markdown,
        b"```tikz\n\\draw (0,0) rectangle (1,1);\n```\n",
    );
    assert!(svg.contains("data-diagram=\"TikZ picture\""), "{svg}");
}

#[test]
fn unknown_format_is_refused_as_a_diagnostic_not_a_path_lookup() {
    let failure = parse_document("sample.bin", ImportFormat::Unknown, b"data").unwrap_err();
    assert_eq!(failure.code, "VUTEX_INPUT_FORMAT_UNSUPPORTED");
    assert_eq!(failure.severity, "error");
}

#[test]
fn gfm_footnote_definition_keeps_its_continuation_paragraphs() {
    let source = b"# Notes\n\nBody text.[^1]\n\n[^1]: First paragraph.\n\n    Second paragraph.\n\n    Third paragraph.\n";
    let (svg, payload) = svg_of("notes.md", ImportFormat::Markdown, source);
    let first = svg.find("[1] First paragraph.").unwrap_or_else(|| panic!("{svg}"));
    let second = svg.find("Second paragraph.").unwrap_or_else(|| panic!("{svg}"));
    let third = svg.find("Third paragraph.").unwrap_or_else(|| panic!("{svg}"));
    assert!(first < second && second < third, "order first={first} second={second} third={third}");
    assert_eq!(payload["footnotes"]["definitions"], 1, "{payload}");
    assert_eq!(payload["footnotes"]["notes"], 1, "{payload}");
}

#[test]
fn gfm_footnote_reference_reaches_a_marker_and_an_endnote_body() {
    let (svg, payload) = svg_of(
        "sample.md",
        ImportFormat::Markdown,
        b"# Notes\n\nBody text.[^1]\n\n[^1]: Footnote body.\n",
    );
    assert!(svg.contains("Body text.[1]"), "{svg}");
    assert!(svg.contains("[1] Footnote body."), "{svg}");
    assert_eq!(payload["footnotes"]["references"], 1, "{payload}");
    assert_eq!(payload["footnotes"]["definitions"], 1, "{payload}");
    assert_eq!(payload["footnotes"]["notes"], 1, "{payload}");
    assert_eq!(payload["diagnostics"].as_array().map(Vec::len), Some(0), "{payload}");
}

#[test]
fn gfm_footnote_marker_survives_the_reflow_html_lane() {
    let parsed = parse_document(
        "sample.md",
        ImportFormat::Markdown,
        b"# Notes\n\nBody text.[^1]\n\n[^1]: Footnote body.\n",
    )
    .expect("parses");
    let lowered = lower_document(parsed.graph, "sample.md");
    let graph = lowered.graph;
    let flow = build_flow(&graph, Theme::Baseline);
    let html = render_flow_html(&flow).expect("html renders");
    assert!(html.contains("[1]"), "{html}");
    assert!(html.contains("Footnote body."), "{html}");
    assert!(html.contains("@font-face"), "reflow html skipped font embedding");
}

#[test]
fn snapshot_html_reaches_a_payload() {
    let parsed = parse_document("sample.md", ImportFormat::Markdown, b"# Title\n\nHello world.\n").expect("parses");
    let lowered = lower_document(parsed.graph, "sample.md");
    let scene = build_display(lowered, Theme::Baseline, None, None);
    let html = render_display_snapshot_html(&scene).expect("snapshot renders");
    assert!(html.contains("<html"), "{html}");
}

#[test]
fn missing_definition_keeps_the_literal_reference_and_reports_it() {
    let (svg, payload) = svg_of("sample.md", ImportFormat::Markdown, b"Body text.[^9]\n");
    assert!(svg.contains("[^9]"), "{svg}");
    let codes = diagnostic_codes(&payload);
    assert!(codes.contains(&FOOTNOTE_DEFINITION_MISSING.to_string()), "{payload}");
}

#[test]
fn repeated_duplicate_and_unreferenced_labels_each_report_their_code() {
    let (svg, payload) = svg_of(
        "sample.md",
        ImportFormat::Markdown,
        b"Body.[^1] again.[^1]\n\n[^1]: Note one.\n\n[^1]: Note one duplicate.\n\n[^2]: Orphan definition.\n",
    );
    assert!(svg.contains("Body.[1] again.[1]"), "{svg}");
    assert!(svg.contains("[1] Note one."), "{svg}");
    assert!(!svg.contains("Orphan definition"), "{svg}");
    let codes = diagnostic_codes(&payload);
    for code in [FOOTNOTE_DEFINITION_DUPLICATE, FOOTNOTE_DEFINITION_UNREFERENCED, FOOTNOTE_REFERENCE_REPEATED] {
        assert!(codes.contains(&code.to_string()), "{code} missing from {payload}");
    }
}

#[test]
fn report_assembly_wraps_caller_sections_without_inventing_status() {
    let report: Value = serde_json::from_str(&assemble_report(
        "render",
        r#"[{"path":"sample.md","format":"markdown"}]"#,
        "[]",
        Some(r#"{"format":"svg","bytes":123}"#),
        Some(r#"{"footnotes":{"references":1,"definitions":1,"notes":1}}"#),
    ))
    .expect("report is JSON");
    assert_eq!(report["schema_version"], "vutex.report/v0");
    assert_eq!(report["operation"], "render");
    assert_eq!(report["inputs"][0]["format"], "markdown");
    assert_eq!(report["output"]["bytes"], 123);
    assert_eq!(report["footnotes"]["notes"], 1);
    assert!(report.get("status").is_none(), "the library must not invent a process status");
}

/// Collect the diagnostic codes of one payload in emission order.
fn diagnostic_codes(payload: &Value) -> Vec<String> {
    payload["diagnostics"]
        .as_array()
        .map(|entries| entries.iter().filter_map(|entry| entry["code"].as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}
