//! PDF backend tests: file structure, xref integrity, and painted primitives.

use vutex_presentation::display::{
    DisplayImage, DisplayItem, DisplayPage, DisplayPresentation, DisplayRect, DisplayRule, DisplayTextRun,
};
use vutex_render_pdf::render_pdf;
use vutex_types::math::FontStyle;

fn page(width: f64, height: f64, items: Vec<DisplayItem>) -> DisplayPage {
    DisplayPage { width, height, items }
}

fn text(x: f64, y: f64, size: f64, family: &str, value: &str) -> DisplayItem {
    DisplayItem::TextRun(DisplayTextRun {
        x,
        y,
        size,
        font_family: family.to_string(),
        style: FontStyle::Normal,
        text: value.to_string(),
        href: None,
    })
}

fn full_page() -> DisplayPage {
    page(
        595.0,
        842.0,
        vec![
            text(72.0, 100.0, 12.0, "serif", "Hello PDF"),
            DisplayItem::Rule(DisplayRule { x1: 72.0, y1: 120.0, x2: 523.0, y2: 120.0, thickness: 0.5 }),
            DisplayItem::Rect(DisplayRect { x: 72.0, y: 140.0, width: 200.0, height: 40.0, filled: true }),
            DisplayItem::Rect(DisplayRect { x: 72.0, y: 200.0, width: 200.0, height: 40.0, filled: false }),
            DisplayItem::Image(DisplayImage { x: 72.0, y: 260.0, width: 120.0, height: 60.0, alt: "figure".to_string() }),
        ],
    )
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|window| window == needle)
}

fn as_str(bytes: &[u8]) -> &str {
    std::str::from_utf8(bytes).expect("pdf payload after the header must be ascii")
}

/// Text of the whole file; the binary comment bytes are replaced lossily.
fn lossy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Byte offset of the `xref` keyword, read from `startxref`.
fn startxref_offset(bytes: &[u8]) -> usize {
    let marker = b"startxref\n";
    let at = find(bytes, marker).expect("startxref keyword");
    let digits = &bytes[at + marker.len()..];
    let end = digits.iter().position(|byte| !byte.is_ascii_digit()).expect("startxref number");
    as_str(&digits[..end]).parse().expect("startxref offset as usize")
}

/// All `N 0 obj` numbers in document order.
fn object_numbers(bytes: &[u8]) -> Vec<usize> {
    let needle = b" 0 obj";
    let mut numbers = Vec::new();
    let mut cursor = 0;
    while let Some(found) = find(&bytes[cursor..], needle) {
        let end = cursor + found;
        let mut start = end;
        while start > 0 && bytes[start - 1].is_ascii_digit() {
            start -= 1;
        }
        numbers.push(as_str(&bytes[start..end]).parse().expect("object number"));
        cursor = end + needle.len();
    }
    numbers
}

/// Xref entry offsets (skipping the free head).
fn xref_entries(bytes: &[u8]) -> Vec<usize> {
    let offset = startxref_offset(bytes);
    let mut lines = as_str(&bytes[offset..]).lines();
    assert_eq!(lines.next(), Some("xref"), "startxref must point at the xref keyword");
    let header = lines.next().expect("xref subsection header");
    let size: usize = header.split_whitespace().nth(1).expect("xref subsection size").parse().unwrap();
    assert_eq!(lines.next(), Some("0000000000 65535 f "), "entry 0 is the free head");
    (1..size)
        .map(|_| {
            let entry = lines.next().expect("xref entry");
            assert_eq!(entry.len(), 19, "every xref entry is 20 bytes including the line feed");
            entry[..10].parse().expect("xref entry offset")
        })
        .collect()
}

fn trailer_size(bytes: &[u8]) -> usize {
    let text = lossy(bytes);
    let at = text.find("/Size ").expect("/Size in trailer");
    let digits: String = text[at + 6..].chars().take_while(char::is_ascii_digit).collect();
    digits.parse().expect("/Size value")
}

#[test]
fn writes_header_and_trailer() {
    let bytes = render_pdf(&DisplayPresentation { pages: vec![full_page()] }).expect("pdf render");

    assert!(bytes.starts_with(b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n"), "header plus binary comment line");
    assert!(bytes.ends_with(b"%%EOF\n"), "pdf trailer terminator");
}

#[test]
fn renders_all_primitives_on_one_page() {
    let bytes = render_pdf(&DisplayPresentation { pages: vec![full_page()] }).expect("pdf render");
    let text = lossy(&bytes);

    assert_eq!(text.matches("/Type /Page ").count(), 1, "one page object");
    assert_eq!(text.matches("/Type /Catalog").count(), 1, "one catalog object");
    assert!(text.contains("/Type /Pages"));
    assert!(text.contains("(Hello PDF) Tj"), "text run with escaped literal string");
    assert!(text.contains("\nS\n"), "rule stroke operator");
    assert!(text.contains("\nf\n"), "filled rect operator");
    assert!(text.contains("[3 2] 0 d"), "image placeholder dash array");
    assert!(text.contains("stream\n") && text.contains("endstream"));
}

#[test]
fn xref_offsets_point_at_object_starts() {
    let bytes =
        render_pdf(&DisplayPresentation { pages: vec![full_page(), page(612.0, 792.0, Vec::new())] }).expect("pdf render");
    let xref_at = startxref_offset(&bytes);
    assert!(bytes[xref_at..].starts_with(b"xref"), "startxref must point at the xref keyword");

    let entries = xref_entries(&bytes);
    let numbers = object_numbers(&bytes);
    assert_eq!(entries.len(), numbers.len(), "one xref entry per object");
    for (index, offset) in entries.iter().enumerate() {
        let expected = format!("{} 0 obj", index + 1);
        assert!(bytes[*offset..].starts_with(expected.as_bytes()), "object {expected} at offset {offset}");
    }
    assert!(xref_at > *entries.last().expect("last entry"), "xref table follows every object");
}

#[test]
fn trailer_size_matches_object_count() {
    let bytes =
        render_pdf(&DisplayPresentation { pages: vec![full_page(), page(612.0, 792.0, Vec::new())] }).expect("pdf render");

    let highest = object_numbers(&bytes).into_iter().max().expect("at least one object");
    assert_eq!(trailer_size(&bytes), highest + 1, "/Size is the highest object number plus one");
}

#[test]
fn two_pages_get_distinct_media_boxes() {
    let presentation = DisplayPresentation {
        pages: vec![page(595.0, 842.0, vec![text(72.0, 100.0, 12.0, "serif", "First")]), page(612.0, 792.0, Vec::new())],
    };
    let bytes = render_pdf(&presentation).expect("pdf render");
    let text = lossy(&bytes);

    assert_eq!(text.matches("/Type /Page ").count(), 2, "two page objects");
    assert!(text.contains("/Count 2"), "/Count follows the page list");
    assert!(text.contains("/MediaBox [0 0 595 842]"));
    assert!(text.contains("/MediaBox [0 0 612 792]"));
}

#[test]
fn substitutes_characters_outside_latin1() {
    let bytes = render_pdf(&DisplayPresentation {
        pages: vec![page(595.0, 842.0, vec![text(72.0, 100.0, 12.0, "sans", "标题 Title")])],
    })
    .expect("pdf render");
    let text = lossy(&bytes);

    assert!(text.contains("(?? Title) Tj"), "non-latin1 characters become question marks");
    assert!(!text.contains("标题"), "no raw utf-8 bytes in winansi content");
}

#[test]
fn maps_typographic_punctuation_to_winansi_bytes() {
    let bytes = render_pdf(&DisplayPresentation {
        pages: vec![page(
            595.0,
            842.0,
            vec![
                text(72.0, 100.0, 12.0, "serif", "\u{2022} item \u{2013} dash"),
                text(72.0, 120.0, 12.0, "serif", "\u{201C}quoted\u{201D} \u{2026} \u{2014} \u{20AC}"),
            ],
        )],
    })
    .expect("pdf render");
    let text = lossy(&bytes);

    assert!(text.contains("(\\225 item \\226 dash) Tj"), "bullet and en dash keep their winansi bytes");
    assert!(text.contains("(\\223quoted\\224 \\205 \\227 \\200) Tj"), "curly quotes, ellipsis, em dash and euro");
    assert!(!text.contains('?'), "representable punctuation never degrades to a question mark");
}

#[test]
fn empty_presentation_is_rejected() {
    let diagnostic = render_pdf(&DisplayPresentation { pages: Vec::new() }).expect_err("empty presentation");

    assert_eq!(diagnostic.code, "vutex.render.pdf.no_pages");
    assert_eq!(diagnostic.severity, vutex_types::Severity::Error);
}
