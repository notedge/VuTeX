//! Integration tests for the document import, layout, and render pipeline.

use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use serde_json::Value;
use vutex_napi::document::{ImportFormat, OutputFormat, inspect_document, render_document};

/// Unique scratch directory per test, removed by [`ScratchDir`] on drop.
struct ScratchDir {
    path: PathBuf,
}

impl ScratchDir {
    fn new(name: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let sequence = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("vutex-napi-{}-{name}-{sequence}", std::process::id()));
        std::fs::create_dir_all(&path).expect("create scratch directory");
        Self { path }
    }

    fn write(&self, file_name: &str, content: &str) -> PathBuf {
        let path = self.path.join(file_name);
        std::fs::write(&path, content).expect("write scratch file");
        path
    }

    fn write_bytes(&self, file_name: &str, content: &[u8]) -> PathBuf {
        let path = self.path.join(file_name);
        std::fs::write(&path, content).expect("write scratch file");
        path
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn report(outcome: &vutex_napi::document::RenderOutcome) -> Value {
    serde_json::from_str(&outcome.report_json).expect("render report is valid JSON")
}

fn inspect_report(outcome: &vutex_napi::document::InspectOutcome) -> Value {
    serde_json::from_str(&outcome.report_json).expect("inspect report is valid JSON")
}

/// Append one PNG chunk with a zero CRC placeholder the decoder ignores.
fn png_chunk(out: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
    out.extend_from_slice(&[0; 4]);
}

/// A 2×2 RGB PNG whose single `IDAT` is one stored DEFLATE block.
fn tiny_png() -> Vec<u8> {
    let scanlines: &[u8] = &[
        0u8, 255, 0, 0, 0, 255, 0, // filter none: red, green
        0, 0, 0, 255, 255, 255, 255, // filter none: blue, white
    ];
    let length = scanlines.len() as u16;
    let mut zlib = vec![0x78, 0x01, 0x01];
    zlib.extend_from_slice(&length.to_le_bytes());
    zlib.extend_from_slice(&(!length).to_le_bytes());
    zlib.extend_from_slice(scanlines);
    zlib.extend_from_slice(&[0; 4]);

    let mut header = Vec::new();
    header.extend_from_slice(&2u32.to_be_bytes());
    header.extend_from_slice(&2u32.to_be_bytes());
    header.extend_from_slice(&[8, 2, 0, 0, 0]);
    let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    png_chunk(&mut png, b"IHDR", &header);
    png_chunk(&mut png, b"IDAT", &zlib);
    png_chunk(&mut png, b"IEND", &[]);
    png
}

/// Encode bytes with the standard base64 alphabet.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let first = u32::from(chunk[0]);
        let second = u32::from(chunk.get(1).copied().unwrap_or(0));
        let third = u32::from(chunk.get(2).copied().unwrap_or(0));
        let value = (first << 16) | (second << 8) | third;
        out.push(char::from(ALPHABET[(value >> 18) as usize & 63]));
        out.push(char::from(ALPHABET[(value >> 12) as usize & 63]));
        out.push(if chunk.len() > 1 { char::from(ALPHABET[(value >> 6) as usize & 63]) } else { '=' });
        out.push(if chunk.len() > 2 { char::from(ALPHABET[value as usize & 63]) } else { '=' });
    }
    out
}

const SAMPLE: &str = "# 标题\n\n第一段正文。\n\n- 项目一\n- 项目二\n";

const INSPECT_SAMPLE: &str = "# 标题\n\n第一段正文。\n\n1. 项目一\n2. 项目二\n";

const RICH_SAMPLE: &str = r##"# 文档标题

> 引用内容

- 项目一
- 项目二

| 指标 | 数值 |
| --- | --- |
| 准确率 | 0.9 |

```rust
fn main() {
    let value = 1;
}
```
"##;

#[test]
fn markdown_renders_svg_with_page_statistics() {
    let dir = ScratchDir::new("svg");
    let path = dir.write("sample.md", SAMPLE);

    let outcome = render_document(&path_string(&path), None);
    let payload = report(&outcome);

    assert_eq!(outcome.exit_code, 0, "{}", outcome.report_json);
    assert_eq!(outcome.output_format, "svg");
    assert!(outcome.html.is_none());
    let svg = outcome.svg.expect("svg payload");
    assert!(svg.contains("<svg"), "{svg}");
    assert_eq!(payload["status"], "success");
    assert!(payload["output"]["pages"].as_u64().unwrap_or(0) >= 1, "{payload}");
    assert!(payload["output"]["items"].as_u64().unwrap_or(0) > 0, "{payload}");
    assert_eq!(payload["output"]["format"], "svg");
    assert_eq!(payload["inputs"][0]["format"], "markdown");
}

#[test]
fn markdown_renders_reflow_html() {
    let dir = ScratchDir::new("html");
    let path = dir.write("sample.md", SAMPLE);

    let outcome = render_document(&path_string(&path), Some(r#"{"output":"html"}"#));
    let payload = report(&outcome);

    assert_eq!(outcome.exit_code, 0, "{}", outcome.report_json);
    assert_eq!(outcome.output_format, "html");
    assert!(outcome.svg.is_none());
    let html = outcome.html.expect("html payload");
    assert!(html.contains("<article"), "{html}");
    assert!(html.contains(r#"data-vutex-block="heading""#), "{html}");
    assert_eq!(payload["output"]["format"], "html");
    assert!(payload["output"]["pages"].as_u64().unwrap_or(0) >= 1, "{payload}");
}

#[test]
fn markdown_renders_snapshot_html() {
    let dir = ScratchDir::new("snapshot");
    let path = dir.write("sample.md", SAMPLE);

    let outcome = render_document(&path_string(&path), Some(r#"{"output":"snapshot"}"#));

    assert_eq!(outcome.exit_code, 0, "{}", outcome.report_json);
    assert_eq!(outcome.output_format, "snapshot");
    assert!(outcome.svg.is_none());
    let html = outcome.html.expect("html payload");
    assert!(html.contains(r#"data-vutex-page="1""#), "{html}");
}

#[test]
fn notedown_file_is_imported_and_rendered() {
    let dir = ScratchDir::new("notedown");
    let path = dir.write("sample.nd", SAMPLE);

    let outcome = render_document(&path_string(&path), None);
    let payload = report(&outcome);

    assert_eq!(outcome.exit_code, 0, "{}", outcome.report_json);
    assert_eq!(payload["inputs"][0]["format"], "notedown");
    assert!(outcome.svg.expect("svg payload").contains("<svg"));
}

#[test]
fn html_file_is_imported_and_rendered() {
    let dir = ScratchDir::new("html-input");
    let path = dir.write("sample.html", "<html><body><h1>Title</h1><p>Hello world.</p></body></html>");

    let outcome = render_document(&path_string(&path), None);
    let payload = report(&outcome);

    assert_eq!(outcome.exit_code, 0, "{}", outcome.report_json);
    assert_eq!(payload["inputs"][0]["format"], "html");
    assert!(outcome.svg.expect("svg payload").contains("<svg"));
}

#[test]
fn tex_file_is_imported_and_rendered() {
    let dir = ScratchDir::new("tex-input");
    let path = dir.write("sample.tex", "\\section{Introduction}\n\nHello world.\n");

    let outcome = render_document(&path_string(&path), None);
    let payload = report(&outcome);

    assert_eq!(outcome.exit_code, 0, "{}", outcome.report_json);
    assert_eq!(payload["inputs"][0]["format"], "tex");
    let svg = outcome.svg.expect("svg payload");
    assert!(svg.contains("<svg"), "{svg}");
    assert!(svg.contains("Introduction"), "{svg}");
}

#[test]
fn typst_file_is_imported_and_rendered() {
    let dir = ScratchDir::new("typst-input");
    let path = dir.write("sample.typ", "= Introduction\n\nHello world.\n");

    let outcome = render_document(&path_string(&path), None);
    let payload = report(&outcome);

    assert_eq!(outcome.exit_code, 0, "{}", outcome.report_json);
    assert_eq!(payload["inputs"][0]["format"], "typst");
    let svg = outcome.svg.expect("svg payload");
    assert!(svg.contains("<svg"), "{svg}");
    assert!(svg.contains("Introduction"), "{svg}");
}

#[test]
fn markdown_renders_pdf_bytes() {
    let dir = ScratchDir::new("pdf");
    let path = dir.write("sample.md", SAMPLE);

    let outcome = render_document(&path_string(&path), Some(r#"{"output":"pdf"}"#));
    let payload = report(&outcome);

    assert_eq!(outcome.exit_code, 0, "{}", outcome.report_json);
    assert_eq!(outcome.output_format, "pdf");
    assert!(outcome.svg.is_none());
    assert!(outcome.html.is_none());
    let pdf = outcome.pdf.expect("pdf payload");
    assert!(!pdf.is_empty(), "pdf payload is never empty");
    assert!(pdf.starts_with(b"%PDF-1.7"), "pdf header: {:?}", &pdf[..pdf.len().min(16)]);
    assert!(pdf.ends_with(b"%%EOF\n"), "pdf trailer terminator");
    assert_eq!(payload["output"]["format"], "pdf");
    assert!(payload["output"]["pages"].as_u64().unwrap_or(0) >= 1, "{payload}");
    assert!(payload["output"]["items"].as_u64().unwrap_or(0) > 0, "{payload}");
}

#[test]
fn markdown_inline_data_uri_image_reaches_the_pdf_as_a_raster() {
    let dir = ScratchDir::new("pdf-inline-image");
    let href = format!("data:image/png;base64,{}", base64(&tiny_png()));
    let path = dir.write("image.md", &format!("# 图片\n\n![图]({href})\n"));

    let outcome = render_document(&path_string(&path), Some(r#"{"output":"pdf"}"#));
    assert_eq!(outcome.exit_code, 0, "{}", outcome.report_json);
    let pdf = outcome.pdf.expect("pdf payload");
    let text = String::from_utf8_lossy(&pdf);

    assert!(text.contains("/Type /XObject /Subtype /Image"), "the inline picture is embedded: {text}");
    assert!(text.contains("\n/Im1 Do\n"), "the page paints the picture: {text}");
    assert!(text.contains("/XObject << /Im1 "), "page resources register the picture: {text}");
    assert!(!text.contains("[3 2] 0 d"), "the inline picture no longer paints the placeholder dash array: {text}");
}

#[test]
fn markdown_inline_svg_image_reaches_the_pdf_as_vector_geometry() {
    let dir = ScratchDir::new("pdf-inline-svg");
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24"><rect width="24" height="24" fill="#ff0000"/><circle cx="12" cy="12" r="6" fill="#000"/></svg>"##;
    let href = format!("data:image/svg+xml;base64,{}", base64(svg.as_bytes()));
    let path = dir.write("icon.md", &format!("# 图标\n\n![图标]({href})\n"));

    let outcome = render_document(&path_string(&path), Some(r#"{"output":"pdf"}"#));
    assert_eq!(outcome.exit_code, 0, "{}", outcome.report_json);
    let pdf = outcome.pdf.expect("pdf payload");
    let text = String::from_utf8_lossy(&pdf);

    assert!(text.contains("1 0 0 rg"), "the icon fill reaches the page: {text}");
    assert!(text.contains("\nf\n"), "the icon paints vector geometry: {text}");
    assert!(!text.contains("/Subtype /Image"), "an SVG icon registers no raster: {text}");
    assert!(!text.contains("[3 2] 0 d"), "the icon no longer paints the placeholder dash array: {text}");
}

#[test]
fn relative_sibling_image_resolves_against_the_document_directory() {
    let dir = ScratchDir::new("pdf-relative-image");
    dir.write_bytes("figure.png", &tiny_png());
    let path = dir.write("image.md", "# 图片\n\n![图](figure.png)\n");

    let outcome = render_document(&path_string(&path), Some(r#"{"output":"pdf"}"#));
    assert_eq!(outcome.exit_code, 0, "{}", outcome.report_json);
    let pdf = outcome.pdf.expect("pdf payload");
    let text = String::from_utf8_lossy(&pdf);

    assert!(text.contains("/Subtype /Image"), "the sibling picture is embedded: {text}");
    assert!(text.contains("\n/Im1 Do\n"), "the page paints the picture: {text}");
    assert!(!text.contains("[3 2] 0 d"), "the resolved sibling replaces the placeholder: {text}");
}

#[test]
fn supplied_resource_payload_embeds_a_remote_image() {
    let dir = ScratchDir::new("pdf-supplied-image");
    let path = dir.write("remote.md", "# 图片\n\n![图](https://example.com/figure.png)\n");
    let options = serde_json::json!({
        "output": "pdf",
        "resources": { "https://example.com/figure.png": base64(&tiny_png()) },
    })
    .to_string();

    let outcome = render_document(&path_string(&path), Some(&options));
    assert_eq!(outcome.exit_code, 0, "{}", outcome.report_json);
    let pdf = outcome.pdf.expect("pdf payload");
    let text = String::from_utf8_lossy(&pdf);

    assert!(text.contains("/Subtype /Image"), "the supplied payload is embedded: {text}");
    assert!(text.contains("\n/Im1 Do\n"), "the page paints the supplied picture: {text}");
    assert!(!text.contains("[3 2] 0 d"), "a supplied remote source replaces the placeholder: {text}");
}

#[test]
fn pdf_keeps_bullet_markers_and_typographic_punctuation() {
    let dir = ScratchDir::new("pdf-winansi");
    let path = dir.write("winansi.md", "- alpha\n- beta\n\n\u{201C}quoted\u{201D} \u{2013} \u{2014} \u{2026}\n");

    let outcome = render_document(&path_string(&path), Some(r#"{"output":"pdf"}"#));
    assert_eq!(outcome.exit_code, 0, "{}", outcome.report_json);
    let pdf = outcome.pdf.expect("pdf payload");
    let text = String::from_utf8_lossy(&pdf);

    assert!(text.contains("(\\225) Tj"), "list markers paint the winansi bullet byte: {text}");
    assert!(text.contains("\\223quoted\\224"), "curly quotes keep their winansi bytes");
    assert!(!text.contains("(?) Tj"), "no marker degrades to a question mark");
}

const LINK_AND_RULE_SAMPLE: &str = "# 标题\n\n正文 [站点](https://example.com) 继续\n\n---\n\n- 项目\n";

#[test]
fn thematic_break_and_external_link_reach_reflow_snapshot_and_svg() {
    let dir = ScratchDir::new("rule-link");
    let path = dir.write("rule-link.md", LINK_AND_RULE_SAMPLE);

    let reflow = render_document(&path_string(&path), Some(r#"{"output":"html"}"#));
    assert_eq!(reflow.exit_code, 0, "{}", reflow.report_json);
    let html = reflow.html.expect("html payload");
    assert!(html.contains(r#"data-vutex-block="rule""#), "{html}");
    assert!(html.contains(r#"<a href="https://example.com">站点</a>"#), "{html}");

    let snapshot = render_document(&path_string(&path), Some(r#"{"output":"snapshot"}"#));
    assert_eq!(snapshot.exit_code, 0, "{}", snapshot.report_json);
    let snapshot_html = snapshot.html.expect("snapshot payload");
    assert!(snapshot_html.contains(r#"<a href="https://example.com">站点</a>"#), "{snapshot_html}");

    let svg = render_document(&path_string(&path), None).svg.expect("svg payload");
    assert!(svg.contains(r#"<a href="https://example.com"><text"#), "{svg}");
}

#[test]
fn missing_file_reports_input_not_found() {
    let dir = ScratchDir::new("missing");
    let path = path_string(&dir.path.join("absent.md"));

    let outcome = render_document(&path, None);

    assert_eq!(outcome.exit_code, 1);
    assert!(outcome.svg.is_none());
    assert!(outcome.html.is_none());
    assert!(outcome.report_json.contains("VUTEX_INPUT_NOT_FOUND"), "{}", outcome.report_json);
}

#[test]
fn unsupported_extension_reports_input_format_unsupported() {
    let dir = ScratchDir::new("unsupported");
    let path = dir.write("sample.xyz", "plain text");

    let outcome = render_document(&path_string(&path), None);

    assert_eq!(outcome.exit_code, 1);
    assert!(outcome.report_json.contains("VUTEX_INPUT_FORMAT_UNSUPPORTED"), "{}", outcome.report_json);
}

#[test]
fn unsupported_output_reports_output_format_unsupported() {
    let dir = ScratchDir::new("bad-output");
    let path = dir.write("sample.md", SAMPLE);

    let outcome = render_document(&path_string(&path), Some(r#"{"output":"docx"}"#));

    assert_eq!(outcome.exit_code, 1);
    assert!(outcome.pdf.is_none());
    assert!(outcome.report_json.contains("VUTEX_OUTPUT_FORMAT_UNSUPPORTED"), "{}", outcome.report_json);
}

#[test]
fn input_override_selects_the_import_adapter() {
    let dir = ScratchDir::new("override");
    let path = dir.write("sample.txt", SAMPLE);

    let outcome = render_document(&path_string(&path), Some(r#"{"input":"markdown"}"#));
    let payload = report(&outcome);

    assert_eq!(outcome.exit_code, 0, "{}", outcome.report_json);
    assert_eq!(payload["inputs"][0]["format"], "markdown");
}

#[test]
fn inspect_reports_document_structure() {
    let dir = ScratchDir::new("inspect");
    let path = dir.write("sample.md", INSPECT_SAMPLE);

    let outcome = inspect_document(&path_string(&path));
    let payload = inspect_report(&outcome);

    assert_eq!(outcome.input_format, "markdown");
    assert_eq!(payload["status"], "success");
    assert_eq!(payload["inputs"][0]["format"], "markdown");
    assert!(payload["document"]["blocks"].as_u64().unwrap_or(0) >= 3, "{payload}");
    assert!(payload["document"]["sections"].as_u64().unwrap_or(0) >= 1, "{payload}");
    assert!(payload["document"]["paragraphs"].as_u64().unwrap_or(0) >= 1, "{payload}");
    assert!(payload["document"]["lists"].as_u64().unwrap_or(0) >= 1, "{payload}");
    assert_eq!(payload["document"]["coverage_complete"], true);
}

#[test]
fn inspect_reports_every_imported_semantic_block() {
    let dir = ScratchDir::new("inspect-rich");
    let path = dir.write("rich.md", RICH_SAMPLE);

    let outcome = inspect_document(&path_string(&path));
    let payload = inspect_report(&outcome);

    assert_eq!(payload["status"], "success");
    let document = &payload["document"];
    assert_eq!(document["sections"], 1, "{payload}");
    assert_eq!(document["quotes"], 1, "{payload}");
    assert_eq!(document["lists"], 1, "{payload}");
    assert_eq!(document["tables"], 1, "{payload}");
    assert_eq!(document["code"], 1, "{payload}");
    assert_eq!(document["coverage_complete"], true, "{payload}");
}

#[test]
fn markdown_semantic_blocks_reach_reflow_html_and_svg() {
    let dir = ScratchDir::new("rich-render");
    let path = dir.write("rich.md", RICH_SAMPLE);

    let html_outcome = render_document(&path_string(&path), Some(r#"{"output":"html"}"#));
    assert_eq!(html_outcome.exit_code, 0, "{}", html_outcome.report_json);
    let html = html_outcome.html.expect("html payload");
    for role in ["quote", "list", "table", "code"] {
        assert!(html.contains(&format!(r#"data-vutex-block="{role}""#)), "missing {role} block: {html}");
    }
    assert!(html.contains("准确率") && html.contains("0.9"), "{html}");
    assert!(html.contains("let value = 1;"), "{html}");

    let svg_outcome = render_document(&path_string(&path), None);
    assert_eq!(svg_outcome.exit_code, 0, "{}", svg_outcome.report_json);
    let svg = svg_outcome.svg.expect("svg payload");
    assert!(svg.contains("引用内容"), "{svg}");
    assert!(svg.contains("准确率"), "{svg}");
    assert!(svg.contains(r#"font-family="monospace""#), "{svg}");
    assert!(svg.contains("let value = 1;"), "{svg}");
}

const MERMAID_SAMPLE: &str = r##"# 流程图

```mermaid
flowchart TD
  A[Start] --> B{Ready?}
  B -->|yes| C[Stop]
  B -->|no| D[Retry]
```
"##;

const BROKEN_MERMAID_SAMPLE: &str = r##"# 流程图

```mermaid
sequenceDiagram
  Alice->>Bob: Hello
```
"##;

const CLASS_MERMAID_SAMPLE: &str = r##"# 类图

```mermaid
classDiagram
  Animal <|-- Duck
```
"##;

#[test]
fn mermaid_code_block_renders_as_a_diagram_scene() {
    let dir = ScratchDir::new("mermaid");
    let path = dir.write("flow.md", MERMAID_SAMPLE);

    let outcome = render_document(&path_string(&path), None);
    let payload = report(&outcome);

    assert_eq!(outcome.exit_code, 0, "{}", outcome.report_json);
    assert_eq!(payload["diagnostics"], serde_json::json!([]), "{payload}");
    let svg = outcome.svg.expect("svg payload");
    assert!(svg.contains(r#"data-diagram="Mermaid diagram""#), "{svg}");
    assert!(svg.contains(">Start</text>"), "{svg}");
    assert!(svg.contains(">Stop</text>"), "{svg}");
    assert!(!svg.contains("flowchart TD"), "the lowered scene replaces the fence text: {svg}");
}

#[test]
fn mermaid_diagram_reaches_the_pdf_as_vector_geometry() {
    let dir = ScratchDir::new("mermaid-pdf");
    let path = dir.write("flow.md", MERMAID_SAMPLE);

    let outcome = render_document(&path_string(&path), Some(r#"{"output":"pdf"}"#));
    let payload = report(&outcome);

    assert_eq!(outcome.exit_code, 0, "{}", outcome.report_json);
    assert_eq!(payload["diagnostics"], serde_json::json!([]), "{payload}");
    let pdf = outcome.pdf.expect("pdf payload");
    let text = String::from_utf8_lossy(&pdf);

    assert!(pdf.starts_with(b"%PDF-1.7\n"), "pdf header");
    assert!(text.ends_with("%%EOF\n"), "pdf trailer");
    assert!(text.contains("/Type /Page "), "the page object exists");
    assert!(text.contains("startxref"), "the xref table closes the file");
    assert!(text.contains("\nW n\n"), "the scene clips to its placed box: {text}");
    assert!(text.contains("(Start) Tj") && text.contains("(Stop) Tj"), "node labels are painted: {text}");
    assert!(text.contains("\nf\n") || text.contains("\nB\n"), "node fills reached the page: {text}");
    assert!(!text.contains("flowchart TD"), "the lowered scene replaces the fence text: {text}");
}

const TIKZ_SAMPLE: &str = r##"# TikZ

```tikz
\begin{tikzpicture}[scale=1.5]
\draw[thick, blue] (0,0) -- (2,1) -- (4,0);
\fill[red] (0,0) rectangle (1,1);
\filldraw[green, dashed] (3,0) circle (0.5);
\draw (0,0) -- (2,0) node[above] {Peak} -- (3,1);
\end{tikzpicture}
```

```tikz
\begin{tikzpicture}
\draw[very thick, dashed, opacity=0.7] (0,0) rectangle (2,1);
\end{tikzpicture}
```
"##;

const BROKEN_TIKZ_SAMPLE: &str = r##"# TikZ

```tikz
\begin{tikzpicture}
\draw[foo] (0,0) -- (1,0);
\end{tikzpicture}
```
"##;

const TEX_TIKZ_SAMPLE: &str = r##"\section{TikZ}

\begin{tikzpicture}
\draw[blue] (0,0) -- (3,1);
\fill[red] (1,0) rectangle (2,1);
\end{tikzpicture}
"##;

#[test]
fn tikz_code_block_renders_as_a_diagram_scene() {
    let dir = ScratchDir::new("tikz");
    let path = dir.write("picture.md", TIKZ_SAMPLE);

    let outcome = render_document(&path_string(&path), None);
    let payload = report(&outcome);

    assert_eq!(outcome.exit_code, 0, "{}", outcome.report_json);
    assert_eq!(payload["diagnostics"], serde_json::json!([]), "{payload}");
    let svg = outcome.svg.expect("svg payload");
    assert_eq!(svg.matches(r#"data-diagram="TikZ picture""#).count(), 2, "both pictures become graphic scenes: {svg}");
    assert!(svg.contains(">Peak</text>"), "the node label is painted: {svg}");
    assert!(!svg.contains("tikzpicture"), "the lowered scene replaces the fence text: {svg}");
    assert!(!svg.contains(r"\draw"), "no raw TikZ source reaches the output: {svg}");
}

#[test]
fn tikz_picture_reaches_the_pdf_as_vector_geometry() {
    let dir = ScratchDir::new("tikz-pdf");
    let path = dir.write("picture.md", TIKZ_SAMPLE);

    let outcome = render_document(&path_string(&path), Some(r#"{"output":"pdf"}"#));
    let payload = report(&outcome);

    assert_eq!(outcome.exit_code, 0, "{}", outcome.report_json);
    assert_eq!(payload["diagnostics"], serde_json::json!([]), "{payload}");
    let pdf = outcome.pdf.expect("pdf payload");
    let text = String::from_utf8_lossy(&pdf);

    assert!(pdf.starts_with(b"%PDF-1.7\n"), "pdf header");
    assert!(text.ends_with("%%EOF\n"), "pdf trailer");
    assert!(text.contains("\nW n\n"), "the scene clips to its placed box: {text}");
    assert!(text.contains("(Peak) Tj"), "node labels are painted: {text}");
    assert!(text.contains("\nf\n") || text.contains("\nB\n"), "fill geometry reached the page: {text}");
    assert!(text.contains("\nS\n"), "stroke geometry reached the page: {text}");
    assert!(!text.contains("tikzpicture"), "the lowered scene replaces the fence text: {text}");
}

#[test]
fn tex_tikzpicture_environment_renders_as_a_diagram_scene() {
    let dir = ScratchDir::new("tikz-tex");
    let path = dir.write("picture.tex", TEX_TIKZ_SAMPLE);

    let outcome = render_document(&path_string(&path), None);
    let payload = report(&outcome);

    assert_eq!(outcome.exit_code, 0, "{}", outcome.report_json);
    assert_eq!(payload["diagnostics"], serde_json::json!([]), "{payload}");
    let svg = outcome.svg.expect("svg payload");
    assert!(svg.contains(r#"data-diagram="TikZ picture""#), "the TeX picture becomes a scene: {svg}");
    assert!(!svg.contains("tikzpicture"), "the lowered scene replaces the environment source: {svg}");
}

#[test]
fn unsupported_tikz_keeps_the_fence_and_reports_a_diagnostic() {
    let dir = ScratchDir::new("tikz-broken");
    let path = dir.write("broken.md", BROKEN_TIKZ_SAMPLE);

    let outcome = render_document(&path_string(&path), None);
    let payload = report(&outcome);

    assert_eq!(outcome.exit_code, 0, "{}", outcome.report_json);
    let svg = outcome.svg.expect("svg payload");
    assert!(svg.contains("draw[foo]"), "the authored fence text stays visible: {svg}");
    assert!(!svg.contains("data-diagram="), "a failed lowering invents no geometry: {svg}");

    let diagnostics = payload["diagnostics"].as_array().expect("diagnostics array");
    assert_eq!(diagnostics.len(), 1, "{payload}");
    assert_eq!(diagnostics[0]["code"], "VUTEX_TIKZ_UNSUPPORTED", "{payload}");
    assert_eq!(diagnostics[0]["severity"], "error", "{payload}");
    assert_eq!(diagnostics[0]["stage"], "diagram", "{payload}");
}

#[test]
fn failed_mermaid_lowering_keeps_the_fence_and_reports_a_diagnostic() {
    let dir = ScratchDir::new("mermaid-broken");
    let path = dir.write("broken.md", BROKEN_MERMAID_SAMPLE);

    let outcome = render_document(&path_string(&path), None);
    let payload = report(&outcome);

    assert_eq!(outcome.exit_code, 0, "{}", outcome.report_json);
    let svg = outcome.svg.expect("svg payload");
    assert!(svg.contains("sequenceDiagram"), "the authored fence text stays visible: {svg}");
    assert!(!svg.contains("data-diagram="), "a failed lowering invents no geometry: {svg}");

    let diagnostics = payload["diagnostics"].as_array().expect("diagnostics array");
    assert_eq!(diagnostics.len(), 1, "{payload}");
    assert_eq!(diagnostics[0]["code"], "davinci.diagram.ast-unavailable", "{payload}");
    assert_eq!(diagnostics[0]["severity"], "error", "{payload}");
    assert_eq!(diagnostics[0]["stage"], "diagram", "{payload}");
}

#[test]
fn class_diagram_lowering_keeps_the_fence_and_reports_a_diagnostic() {
    let dir = ScratchDir::new("mermaid-class");
    let path = dir.write("class.md", CLASS_MERMAID_SAMPLE);

    let outcome = render_document(&path_string(&path), None);
    let payload = report(&outcome);

    assert_eq!(outcome.exit_code, 0, "{}", outcome.report_json);
    let svg = outcome.svg.expect("svg payload");
    assert!(svg.contains("classDiagram"), "the authored fence text stays visible: {svg}");
    assert!(!svg.contains("data-diagram="), "an unsupported diagram family invents no geometry: {svg}");

    let diagnostics = payload["diagnostics"].as_array().expect("diagnostics array");
    assert_eq!(diagnostics.len(), 1, "{payload}");
    assert_eq!(diagnostics[0]["code"], "davinci.diagram.ast-unavailable", "{payload}");
    assert_eq!(diagnostics[0]["severity"], "error", "{payload}");
    assert_eq!(diagnostics[0]["stage"], "diagram", "{payload}");
}

#[test]
fn format_ids_match_the_report_contract() {
    assert_eq!(ImportFormat::from_path("a.md").id(), "markdown");
    assert_eq!(ImportFormat::from_path("a.nd").id(), "notedown");
    assert_eq!(ImportFormat::from_path("a.htm").id(), "html");
    assert_eq!(ImportFormat::from_path("a.tex").id(), "tex");
    assert_eq!(ImportFormat::from_path("a.latex").id(), "tex");
    assert_eq!(ImportFormat::from_path("a.typ").id(), "typst");
    assert_eq!(ImportFormat::from_path("a.typst").id(), "typst");
    assert_eq!(ImportFormat::from_option("ltx").id(), "tex");
    assert_eq!(ImportFormat::from_option("Typst").id(), "typst");
    assert_eq!(OutputFormat::from_option("SVG").map(OutputFormat::id), Some("svg"));
    assert_eq!(OutputFormat::from_option("PDF").map(OutputFormat::id), Some("pdf"));
    assert!(OutputFormat::from_option("docx").is_none());
}
