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
use vutex_layout::hint::{ChromeHeader, SectionChrome};
use vutex_napi::{
    document::{
        ImportFormat, TemplateColors, TemplateGeometry, assemble_report, build_display, build_flow, lower_document,
        parse_document, render_display_snapshot_html, render_display_svg, render_flow_html,
    },
    footnote::{
        FOOTNOTE_DEFINITION_DUPLICATE, FOOTNOTE_DEFINITION_MISSING, FOOTNOTE_DEFINITION_UNREFERENCED,
        FOOTNOTE_REFERENCE_REPEATED,
    },
    theme::Theme,
};
use vutex_text::math::MathFontFamily;

/// Configure the repository font pack once for this test binary.
fn configure_fonts() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let fonts = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fonts");
        vutex_napi::configure_font_assets_dir(fonts.display().to_string(), None).expect("configure repository font pack");
    });
}

/// Parse, lower, and emit SVG for one source.
fn svg_of(name: &str, format: ImportFormat, source: &[u8]) -> (String, Value) {
    configure_fonts();
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
    configure_fonts();
    let parsed = parse_document("sample.md", ImportFormat::Markdown, b"# One\n\nBody.\n\n## Sub\n\nMore.\n\n# Two\n\nEnd.\n")
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
fn tex_sample_keeps_profile_constructs_in_svg() {
    let source = br"\section{Introduction}
Body with \textbf{bold} and \href{https://example.com}{link} and \cite{alpha,beta} and \ref{sec:one}.

\begin{tabular}{ll}
Left & Right \\
\end{tabular}

Inline math $x^2 + 1$.

\begin{equation}
\frac{a}{b}
\end{equation}

\begin{foobar}
kept environment body
\end{foobar}

Footnote reference\footnote{note body with $y$}.
";
    let (svg, _payload) = svg_of("sample.tex", ImportFormat::Tex, source);
    assert!(svg.contains("Introduction"), "{svg}");
    assert!(svg.contains("bold"), "{svg}");
    assert!(svg.contains("Left"), "{svg}");
    assert!(svg.contains("Right"), "{svg}");
    assert!(svg.contains("kept environment body"), "the unknown environment must stay visible: {svg}");
    assert!(svg.contains("note body"), "{svg}");
    assert!(svg.contains("https://example.com"), "the link target must survive: {svg}");
    assert!(!svg.contains("$x^2 + 1$"), "inline math must be painted, not printed as source: {svg}");
    assert!(!svg.contains("\\frac{a}{b}"), "block math must be painted, not printed as source: {svg}");
    assert!(!svg.contains("(1)"), "an unnumbered equation must not print a number: {svg}");
}

#[test]
fn tex_profile_report_reaches_the_lowering_payload() {
    let source = b"\\section{Intro}\n\n\\begin{foobar}\nkept\n\\end{foobar}\n";
    let parsed = parse_document("sample.tex", ImportFormat::Tex, source).expect("tex source parses");
    let conversion = parsed.conversion.expect("the profile projects a conversion report");
    assert!(
        conversion.unsupported.iter().any(|item| item.code == "import.tex.unknown_environment"),
        "{conversion:?}"
    );
    let lowered = lower_document(parsed.graph, "sample.tex");
    assert!(
        !lowered.diagnostics.iter().any(|entry| entry.code == "vutex.layout.math.unsupported_language"),
        "TeX math must reach the shared formula pipeline: {:?}",
        lowered.diagnostics
    );
}

#[test]
fn tikz_fence_reaches_a_diagram_scene() {
    let (svg, _) = svg_of("sample.md", ImportFormat::Markdown, b"```tikz\n\\draw (0,0) rectangle (1,1);\n```\n");
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
    let (svg, payload) = svg_of("sample.md", ImportFormat::Markdown, b"# Notes\n\nBody text.[^1]\n\n[^1]: Footnote body.\n");
    assert!(svg.contains("Body text.[1]"), "{svg}");
    assert!(svg.contains("[1] Footnote body."), "{svg}");
    assert_eq!(payload["footnotes"]["references"], 1, "{payload}");
    assert_eq!(payload["footnotes"]["definitions"], 1, "{payload}");
    assert_eq!(payload["footnotes"]["notes"], 1, "{payload}");
    assert_eq!(payload["diagnostics"].as_array().map(Vec::len), Some(0), "{payload}");
}

#[test]
fn gfm_footnote_body_keeps_its_inline_semantics() {
    let source = b"Body text.[^1]\n\n[^1]: Body with $\\alpha + \\beta$ and `SIGMA13`.\n";
    let (svg, payload) = svg_of("notes.md", ImportFormat::Markdown, source);
    assert!(!svg.contains("$\\alpha"), "inline math stayed literal in the endnote: {svg}");
    assert!(!svg.contains('`'), "inline code kept its authored backticks: {svg}");
    assert!(svg.contains("SIGMA13"), "the note body text itself was dropped: {svg}");
    assert_eq!(payload["footnotes"]["notes"], 1, "{payload}");
}

#[test]
fn gfm_footnote_body_falls_back_to_text_for_block_level_content() {
    let source = b"Body text.[^1]\n\n[^1]: - item\n";
    let (svg, _) = svg_of("notes.md", ImportFormat::Markdown, source);
    assert!(svg.contains("[1] - item"), "a block-level body must stay literal: {svg}");
}

#[test]
fn gfm_footnote_marker_survives_the_reflow_html_lane() {
    configure_fonts();
    let parsed = parse_document("sample.md", ImportFormat::Markdown, b"# Notes\n\nBody text.[^1]\n\n[^1]: Footnote body.\n")
        .expect("parses");
    let lowered = lower_document(parsed.graph, "sample.md");
    let graph = lowered.graph;
    let flow = build_flow(&graph, Theme::Baseline, None);
    let html = render_flow_html(&flow).expect("html renders").html;
    assert!(html.contains("[1]"), "{html}");
    assert!(html.contains("Footnote body."), "{html}");
    assert!(html.contains("@font-face"), "reflow html skipped font embedding");
}

#[test]
fn reflow_html_typesets_formulas_instead_of_printing_tex() {
    configure_fonts();
    let parsed =
        parse_document("sample.md", ImportFormat::Markdown, b"# Title\n\nArea $x^2$.\n\n$$\\frac{a}{b}$$\n").expect("parses");
    let lowered = lower_document(parsed.graph, "sample.md");
    let flow = build_flow(&lowered.graph, Theme::Baseline, Some(MathFontFamily::LmMath));
    let outcome = render_flow_html(&flow).expect("html renders");
    assert!(outcome.html.contains("<span class=\"vutex-math\""), "{}", outcome.html);
    assert!(outcome.html.contains("<div class=\"vutex-math\" data-vutex-block=\"math\""), "{}", outcome.html);
    assert!(!outcome.html.contains("<pre data-vutex-block=\"code\">\\frac"), "{}", outcome.html);
    assert!(outcome.diagnostics.is_empty(), "{:?}", outcome.diagnostics);
}

#[test]
fn snapshot_html_reaches_a_payload() {
    configure_fonts();
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

#[test]
fn tex_source_reports_unknown_environment_through_the_profile() {
    let source = b"\\section{Intro}\n\nBody.\n\n\\begin{foobar}\nkept\n\\end{foobar}\n";
    let parsed = parse_document("sample.tex", ImportFormat::Tex, source).expect("tex source parses");
    assert_eq!(parsed.format.id(), "tex");
    let conversion = parsed.conversion.expect("the tex profile projects a conversion report");
    assert!(
        conversion.unsupported.iter().any(|item| item.code == "import.tex.unknown_environment"),
        "unknown environment missing from {conversion:?}"
    );
    let record = parsed
        .diagnostics
        .iter()
        .find(|record| record.code == "import.tex.unknown_environment")
        .unwrap_or_else(|| panic!("unknown environment diagnostic missing from {:?}", parsed.diagnostics));
    let span = record.span.unwrap_or_else(|| panic!("the profile must record the environment span: {record:?}"));
    let text = std::str::from_utf8(&source[span.start..span.end]).expect("diagnostic span is UTF-8");
    assert_eq!(text, "\\begin{foobar}\nkept\n\\end{foobar}");
}

#[test]
fn tex_source_over_budget_returns_the_profile_code() {
    let source = "x".repeat(8 * 1024 * 1024 + 1);
    let failure = parse_document("sample.tex", ImportFormat::Tex, source.as_bytes()).unwrap_err();
    assert_eq!(failure.code, "vutex.input.tex.source_too_large");
}

#[test]
fn typst_unterminated_group_is_refused_by_the_profile() {
    let source = b"= Intro\n\nBody.\n\n#set page(\n  width: 10cm,\n";
    let failure = parse_document("sample.typ", ImportFormat::Typst, source).unwrap_err();
    assert_eq!(failure.code, "vutex.input.typst.unterminated_dynamic_group");
    assert_eq!(failure.severity, "error");
}

#[test]
fn typst_single_line_group_yields_one_span() {
    let source = "= Intro\n\n#set page(width: 10cm)\n\nBody.\n";
    let parsed = parse_document("sample.typ", ImportFormat::Typst, source.as_bytes()).expect("typst source parses");
    assert_eq!(parsed.diagnostics.len(), 1, "{:?}", parsed.diagnostics);
    let record = &parsed.diagnostics[0];
    assert_eq!(record.code, "import.typst.dynamic_content");
    let span = record.span.unwrap_or_else(|| panic!("the profile must record the group span: {record:?}"));
    assert_eq!(&source[span.start..span.end], "#set page(width: 10cm)");
    let conversion = parsed.conversion.expect("the typst profile projects a conversion report");
    assert!(
        conversion.unsupported.iter().any(|item| item.code == "import.typst.dynamic_content"),
        "{conversion:?}"
    );
}

#[test]
fn typst_source_over_budget_returns_the_profile_code() {
    let source = "x".repeat(8 * 1024 * 1024 + 1);
    let failure = parse_document("sample.typ", ImportFormat::Typst, source.as_bytes()).unwrap_err();
    assert_eq!(failure.code, "vutex.input.typst.source_too_large");
}

#[test]
fn typst_math_is_preserved_and_layout_reports_the_language() {
    let source = "= Intro\n\nMath $x^2 + 1$.\n";
    let parsed = parse_document("sample.typ", ImportFormat::Typst, source.as_bytes()).expect("typst source parses");
    let conversion = parsed.conversion.expect("the typst profile projects a conversion report");
    assert!(
        conversion.preserved.iter().any(|item| item.code == "import.typst.math.source_preserved"),
        "typst math must be preserved: {conversion:?}"
    );
    configure_fonts();
    let lowered = lower_document(parsed.graph, "sample.typ");
    let scene = build_display(lowered, Theme::Baseline, None, None);
    assert!(
        scene.diagnostics.iter().any(|record| record.code == "vutex.layout.math.unsupported_language"),
        "layout must report the unlowered language: {:?}",
        scene.diagnostics
    );
    let svg = render_display_svg(&scene).expect("svg renders");
    assert!(svg.contains("x^2 + 1"), "preserved math source stays visible: {svg}");
}

/// Collect the diagnostic codes of one payload in emission order.
fn diagnostic_codes(payload: &Value) -> Vec<String> {
    payload["diagnostics"]
        .as_array()
        .map(|entries| entries.iter().filter_map(|entry| entry["code"].as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}
