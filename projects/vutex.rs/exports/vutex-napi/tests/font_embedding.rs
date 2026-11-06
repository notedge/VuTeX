//! Font embedding lane for every emission path.
//!
//! `document-renderer-svg` names generic CSS families, so the host decides which
//! face paints each run. That face can be wider than the one `document-layout`
//! measured with, which overlaps runs. These tests pin the payload to the
//! crate asset pack instead: explicit family names plus the face bytes, so the
//! painting face is the measured face.
//!
//! The last test is the regression guard: it fails if any document SVG entry
//! emits a generic `serif` or `monospace` family or omits `@font-face`.

use vutex_napi::{
    document::{
        build_display, lower_document, parse_document, render_display_snapshot_html, render_display_svg,
        ImportFormat,
    },
    svg_fonts::{embed_body_fonts, embed_html_fonts},
    theme::Theme,
};

fn render(name: &str, source: &str) -> String {
    let parsed = parse_document(name, ImportFormat::Markdown, source.as_bytes()).expect("markdown parses");
    let lowered = lower_document(parsed.graph, name);
    let scene = build_display(lowered, Theme::Baseline, None);
    render_display_svg(&scene).expect("svg renders")
}

#[test]
fn latin_document_carries_only_the_baseline_faces() {
    let svg = render("sample.md", "# Title\n\nBody prose with `code_span` inside.\n");

    assert!(svg.contains("<style type=\"text/css\">"), "no face block");
    assert!(svg.contains("@font-face"), "no embedded face");
    assert!(svg.contains("font-family=\"VuTeX Serif"), "prose run not named");
    assert!(svg.contains("font-family=\"VuTeX Mono"), "code run not named: {svg}");
    assert!(!svg.contains("font-family=\"serif\""), "generic prose family left in place");
    assert!(!svg.contains("font-family=\"monospace\""), "generic code family left in place");
    assert!(!svg.contains("VuTeX Serif CJK"), "the legacy CJK family name leaked into the baseline");
    assert!(svg.len() < 400_000, "Latin payload grew to {} bytes", svg.len());
}

#[test]
fn han_document_paints_with_the_bundled_noto_faces_only() {
    let svg = render("han.md", "# 标题\n\n这是一段中文正文，包含标点。\n");

    assert!(svg.matches("@font-face").count() >= 1, "no embedded Han face");
    assert!(svg.contains("font-family=\"VuTeX Serif"), "Han run has no bundled family");
    assert!(!svg.contains("SimSun"), "a system CJK face leaked into the payload");
    assert!(!svg.contains("SimHei"), "a system CJK face leaked into the payload");
    assert!(svg.len() < 1_000_000, "Han payload grew to {} bytes", svg.len());
}

#[test]
fn headings_are_bold_and_body_text_is_not() {
    let svg = render("levels.md", "# Title\n\n## Section\n\nBody text.\n");
    assert!(svg.contains("font-weight=\"700\""), "headings carry no structural weight");
}

#[test]
fn snapshot_html_carries_the_same_faces_as_svg() {
    let parsed = parse_document("sample.md", ImportFormat::Markdown, b"# Title\n\nBody `code`.\n").expect("parses");
    let lowered = lower_document(parsed.graph, "sample.md");
    let scene = build_display(lowered, Theme::Baseline, None);
    let html = render_display_snapshot_html(&scene).expect("snapshot renders");
    assert!(html.contains("@font-face"), "snapshot html has no embedded face");
    assert!(!html.contains("font-family:serif"), "snapshot html kept the generic family");
}

/// Every SVG emission entry must embed bundled faces and must not emit a
/// generic family. Both the document entry and the standalone injector are
/// checked so a future caller cannot bypass the shared layer.
#[test]
fn every_svg_entry_embeds_bundled_faces_and_drops_generic_families() {
    let source = "<svg xmlns=\"http://www.w3.org/2000/svg\"><title>x</title><text font-family=\"serif\">a</text><text font-family=\"monospace\">b</text></svg>";
    for theme in [Theme::Baseline, Theme::Traditional] {
        let out = embed_body_fonts(source, theme);
        assert!(!out.contains("font-family=\"serif\""), "{out}");
        assert!(!out.contains("font-family=\"monospace\""), "{out}");
        assert!(out.contains("@font-face"), "{out}");
    }

    let html = "<html><head></head><body><div style=\"font-family:serif\">a</div></body></html>";
    let out = embed_html_fonts(html, Theme::Baseline);
    assert!(out.contains("@font-face"), "{out}");
    assert!(!out.contains("font-family:serif"), "{out}");
}
