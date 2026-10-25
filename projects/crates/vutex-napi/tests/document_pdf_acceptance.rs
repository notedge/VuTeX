//! End-to-end acceptance for a real document: a Markdown fixture runs through
//! the public `render_document` pipeline and the generated PDF is inspected for
//! painted text, embedded `ToUnicode` maps, and diagram vector operators.
//!
//! The fixture carries Chinese prose, a Rust code fence, Unicode arrows and
//! maths, monochrome emoji, and Mermaid/TikZ/SVG diagrams, so a regression in
//! the fallback font chain or in diagram lowering fails here as it would for a
//! README or architecture document.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use vutex_napi::document::render_document;

fn fixture(name: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name);
    assert!(path.exists(), "the acceptance fixture must ship with the crate: {}", path.display());
    path
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// Render the fixture through the public document pipeline and return its PDF.
fn fixture_pdf() -> Vec<u8> {
    let outcome = render_document(&path_string(&fixture("document-acceptance.md")), Some(r#"{"output":"pdf"}"#));
    assert_eq!(outcome.exit_code, 0, "the fixture must render without diagnostics: {}", outcome.report_json);
    assert_eq!(outcome.output_format, "pdf");
    let pdf = outcome.pdf.expect("pdf bytes");
    assert!(pdf.starts_with(b"%PDF-1.7\n"), "pdf header");
    assert!(pdf.ends_with(b"%%EOF\n"), "pdf trailer");
    pdf
}

fn lossy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Body of one indirect object, between its `N 0 obj` line and `endobj`.
fn object_body(text: &str, number: usize) -> Option<&str> {
    let marker = format!("\n{number} 0 obj\n");
    let start = text.find(&marker)? + marker.len();
    let end = start + text[start..].find("\nendobj")?;
    Some(&text[start..end])
}

/// Payload of one indirect stream object, without its dictionary or markers.
fn stream_payload(text: &str, number: usize) -> Option<&str> {
    let body = object_body(text, number)?;
    let start = body.find("stream\n")? + "stream\n".len();
    let end = body.rfind("\nendstream")?;
    Some(&body[start..end])
}

/// Page content stream object numbers, read from `/Contents N 0 R`.
fn page_content_numbers(text: &str) -> Vec<usize> {
    let mut numbers = Vec::new();
    let mut cursor = 0;
    while let Some(found) = text[cursor..].find("/Contents ") {
        let at = cursor + found + "/Contents ".len();
        let digits: String = text[at..].chars().take_while(char::is_ascii_digit).collect();
        if let Ok(number) = digits.parse() {
            numbers.push(number);
        }
        cursor = at;
    }
    numbers
}

/// Every page content stream, joined so tests can read paint operators alone.
fn page_content(bytes: &[u8]) -> String {
    let text = lossy(bytes);
    page_content_numbers(&text).iter().filter_map(|number| stream_payload(&text, *number)).collect::<Vec<_>>().join("\n")
}

/// Font resource names and the object numbers they point at.
fn font_resources(text: &str) -> BTreeMap<usize, usize> {
    let mut resources = BTreeMap::new();
    let Some(found) = text.find("/Font << ")
    else {
        return resources;
    };
    let start = found + "/Font << ".len();
    let end = start + text[start..].find(" >>").expect("the font resource dictionary closes");
    for entry in text[start..end].split_whitespace().collect::<Vec<_>>().chunks(4) {
        let [name, object, "0", "R"] = entry
        else {
            continue;
        };
        let (Some(resource), Ok(object)) = (name.strip_prefix("/F").and_then(|name| name.parse().ok()), object.parse())
        else {
            continue;
        };
        resources.insert(resource, object);
    }
    resources
}

/// `ToUnicode` CID maps keyed by `/F` resource index, for composite fonts only.
fn embedded_cmaps(text: &str) -> BTreeMap<usize, BTreeMap<u16, char>> {
    let mut maps = BTreeMap::new();
    for (resource, object) in font_resources(text) {
        let Some(body) = object_body(text, object)
        else {
            continue;
        };
        if !body.contains("/Subtype /Type0") {
            continue;
        }
        let Some(found) = body.find("/ToUnicode ")
        else {
            continue;
        };
        let digits: String = body[found + "/ToUnicode ".len()..].chars().take_while(char::is_ascii_digit).collect();
        let Some(map) = digits.parse().ok().and_then(|number| stream_payload(text, number)).map(decode_cmap)
        else {
            continue;
        };
        maps.insert(resource, map);
    }
    maps
}

/// CID-to-character pairs from one `beginbfchar` section of a CMap.
fn decode_cmap(cmap: &str) -> BTreeMap<u16, char> {
    let mut map = BTreeMap::new();
    let mut inside = false;
    for line in cmap.lines() {
        let line = line.trim();
        if line.ends_with("beginbfchar") {
            inside = true;
            continue;
        }
        if line == "endbfchar" {
            inside = false;
            continue;
        }
        let Some((source, target)) = inside.then_some(line).and_then(|line| line.split_once(' '))
        else {
            continue;
        };
        let source = source.trim_matches(|ch| ch == '<' || ch == '>');
        let target = target.trim_matches(|ch| ch == '<' || ch == '>');
        let (Ok(cid), Some(ch)) = (u16::from_str_radix(source, 16), decode_utf16_hex(target))
        else {
            continue;
        };
        map.insert(cid, ch);
    }
    map
}

/// One character from its UTF-16BE hexadecimal form, surrogate pairs included.
fn decode_utf16_hex(hex: &str) -> Option<char> {
    let units: Vec<u16> = hex
        .as_bytes()
        .chunks(4)
        .map(|chunk| u16::from_str_radix(std::str::from_utf8(chunk).ok()?, 16).ok())
        .collect::<Option<_>>()?;
    char::decode_utf16(units).collect::<Result<String, _>>().ok()?.chars().next()
}

/// Characters painted by every page, decoded through each font's `ToUnicode`.
///
/// A CID without a `ToUnicode` entry becomes `U+FFFD` on purpose: a text run
/// whose glyphs cannot be mapped back is a broken document even when the PDF
/// renders, so the acceptance test must fail rather than silently drop it.
fn painted_text(bytes: &[u8]) -> String {
    let text = lossy(bytes);
    let cmaps = embedded_cmaps(&text);
    let mut painted = String::new();
    for number in page_content_numbers(&text) {
        let Some(content) = stream_payload(&text, number)
        else {
            continue;
        };
        let mut font = 0;
        for line in content.lines() {
            if let Some(name) = line.trim_end().strip_suffix(" Tf") {
                if let Some(index) =
                    name.split_whitespace().next().and_then(|name| name.strip_prefix("/F")).and_then(|name| name.parse().ok())
                {
                    font = index;
                }
                continue;
            }
            let Some(shown) = line.trim_end().strip_suffix(" Tj")
            else {
                continue;
            };
            if let Some(hex) = shown.strip_prefix('<').and_then(|shown| shown.strip_suffix('>')) {
                for chunk in hex.as_bytes().chunks(4) {
                    let Some(cid) = u16::from_str_radix(std::str::from_utf8(chunk).unwrap_or(""), 16).ok()
                    else {
                        painted.push('\u{FFFD}');
                        continue;
                    };
                    painted.push(cmaps.get(&font).and_then(|map| map.get(&cid)).copied().unwrap_or('\u{FFFD}'));
                }
            }
            else if let Some(literal) = shown.strip_prefix('(').and_then(|shown| shown.strip_suffix(')')) {
                painted.push_str(&unescape_text(literal));
            }
        }
    }
    painted
}

/// Decode a PDF literal string, including its octal escapes.
fn unescape_text(literal: &str) -> String {
    let mut out = String::new();
    let mut chars = literal.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.peek().copied() {
            Some('\\' | '(' | ')') => out.push(chars.next().expect("an escaped character")),
            Some(digit) if ('0'..='7').contains(&digit) => {
                let mut value = 0;
                for _ in 0..3 {
                    let Some(digit) = chars.peek().copied().filter(|ch| ('0'..='7').contains(ch))
                    else {
                        break;
                    };
                    value = value * 8 + digit.to_digit(8).expect("an octal digit");
                    chars.next();
                }
                out.extend(char::from_u32(value));
            }
            _ => {}
        }
    }
    out
}

/// Every character of the fixture the base-14 encoding cannot paint, in code
/// point order. These are exactly the characters the embedded chain must cover.
fn embedded_fixture_chars() -> BTreeSet<char> {
    let source = std::fs::read_to_string(fixture("document-acceptance.md")).expect("readable fixture");
    source.chars().filter(|ch| (*ch as u32) > 0xFF).collect()
}

#[test]
fn real_document_paints_chinese_code_symbols_and_emoji() {
    let bytes = fixture_pdf();
    let text = painted_text(&bytes);

    assert!(!text.is_empty(), "the document must paint text");
    assert!(!text.contains('\u{FFFD}'), "every painted CID must map back to Unicode: {text}");
    assert!(!text.contains('?'), "no character may fall back to a question mark: {text}");

    // Chinese prose, headings, and diagram labels.
    for expected in ["真实文档验收", "中文", "代码围栏", "渲染入口", "输入", "校验", "通过", "输出", "峰值"]
    {
        assert!(text.contains(expected), "missing Chinese text {expected:?}: {text}");
    }

    // Code fence text, including its Chinese comment and the full-width colon.
    for expected in ["// 中文注释：渲染入口", "fn render() -> usize { 42 }"] {
        assert!(text.contains(expected), "missing code fence line {expected:?}: {text}");
    }

    // Arrows, maths, and monochrome emoji must be painted as real characters.
    for expected in ["→", "←", "↔", "⇒", "⇐", "∑", "∞", "≤", "≥", "∈", "±", "≠", "≈", "√", "✔", "★", "☀"]
    {
        assert!(text.contains(expected), "missing symbol {expected:?}: {text}");
    }

    // The chain must actually cover every non-WinAnsi character the fixture carries.
    let missing: Vec<char> = embedded_fixture_chars().into_iter().filter(|ch| !text.contains(*ch)).collect();
    assert!(missing.is_empty(), "the document dropped characters {missing:?}");

    // Han and symbols cannot come from one face, so the chain must have subsets
    // for both and each must publish a ToUnicode map.
    let cmaps = embedded_cmaps(&lossy(&bytes));
    assert!(cmaps.len() >= 2, "expected disjoint Han and symbol subsets, got {}: {cmaps:?}", cmaps.len());
    assert!(cmaps.values().all(|map| !map.is_empty()), "every embedded subset maps its CIDs back to Unicode");

    // Embedded text is painted as hexadecimal CIDs, never as raw UTF-8 bytes.
    let content = page_content(&bytes);
    assert!(!content.contains("中文"), "Chinese text leaked into the literal-string path: {content}");
}

#[test]
fn real_document_paints_diagram_vectors() {
    let bytes = fixture_pdf();
    let content = page_content(&bytes);

    // Mermaid and TikZ scenes are placed as clipped, transformed vector boxes.
    assert!(content.matches(" re\n").count() >= 3, "clip and fill rectangles must use `re`: {content}");
    assert!(content.matches("W n\n").count() >= 2, "each diagram scene must clip its box: {content}");
    assert!(content.matches(" cm\n").count() >= 3, "each scene must carry a placement transform: {content}");
    for expected in [" m\n", " l\n", " c\n"] {
        assert!(content.contains(expected), "missing path operator {expected:?}: {content}");
    }
    assert!(content.contains("\nf\n") || content.contains("\nB\n"), "diagram fills must use a fill operator: {content}");
    assert!(content.contains("\nS\n"), "diagram strokes must use `S`: {content}");

    // The SVG decoder paints paths, not a raster image or the placeholder box.
    assert!(!content.contains("/Subtype /Image"), "the diagram must stay vector: {content}");
    assert!(!content.contains("[3 2] 0 d"), "the SVG must resolve instead of falling back to the placeholder: {content}");

    // Diagram source stays out of the page: only the lowered scenes paint.
    for source in ["flowchart", "tikzpicture", "\\draw", "rectangle"] {
        assert!(!content.contains(source), "diagram source leaked into the content stream: {source}");
    }

    // Scene labels keep the embedded chain busy even inside graphics.
    let text = painted_text(&bytes);
    for expected in ["输入", "校验", "输出", "峰值"] {
        assert!(text.contains(expected), "missing diagram label {expected:?}: {text}");
    }
}
