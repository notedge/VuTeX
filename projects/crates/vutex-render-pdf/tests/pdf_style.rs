//! PDF tests for styled text runs: base-14 font selection and xref integrity.

use vutex_presentation::display::{DisplayItem, DisplayPage, DisplayPresentation, DisplayTextRun};
use vutex_render_pdf::render_pdf;
use vutex_types::math::FontStyle;

fn text(family: &str, style: FontStyle, value: &str) -> DisplayItem {
    DisplayItem::TextRun(DisplayTextRun {
        x: 72.0,
        y: 100.0,
        size: 12.0,
        font_family: family.to_string(),
        style,
        text: value.to_string(),
        href: None,
    })
}

fn render_bytes(items: Vec<DisplayItem>) -> Vec<u8> {
    render_pdf(&DisplayPresentation { pages: vec![DisplayPage { width: 595.0, height: 842.0, items }] }).expect("pdf render")
}

fn render(items: Vec<DisplayItem>) -> String {
    String::from_utf8_lossy(&render_bytes(items)).into_owned()
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

/// Font objects in object-number order: `(object number, base-14 name)`.
fn font_objects(text: &str) -> Vec<(usize, String)> {
    let needle = " 0 obj\n<< /Type /Font";
    let mut objects = Vec::new();
    let mut cursor = 0;
    while let Some(found) = text[cursor..].find(needle) {
        let end = cursor + found;
        let mut start = end;
        while start > 0 && text.as_bytes()[start - 1].is_ascii_digit() {
            start -= 1;
        }
        let number: usize = text[start..end].parse().expect("font object number");
        let base_at = end + text[end..].find("/BaseFont /").expect("base font entry") + "/BaseFont /".len();
        let name: String = text[base_at..].chars().take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '-').collect();
        objects.push((number, name));
        cursor = end + needle.len();
    }
    objects
}

/// `/Fn` resource entries as `(resource index, font object number)`.
fn font_resources(text: &str) -> Vec<(usize, usize)> {
    let start = text.find("/Font << ").expect("font resource dictionary") + "/Font << ".len();
    let end = start + text[start..].find(" >>").expect("font resource dictionary end");
    text[start..end]
        .split('/')
        .filter(|entry| !entry.trim().is_empty())
        .map(|entry| {
            let tokens: Vec<&str> = entry.split_whitespace().collect();
            let index: usize = tokens[0].trim_start_matches('F').parse().expect("resource index");
            let object: usize = tokens[1].parse().expect("resource object number");
            (index, object)
        })
        .collect()
}

/// `(/Fn index, size)` for every `Tf` operator in content stream order.
fn font_uses(text: &str) -> Vec<(usize, f64)> {
    let mut uses = Vec::new();
    let mut cursor = 0;
    while let Some(found) = text[cursor..].find(" Tf") {
        let end = cursor + found;
        let before = &text[..end];
        let size_start = before.rfind(|ch: char| !(ch.is_ascii_digit() || ch == '.')).map(|index| index + 1).unwrap_or(0);
        let size: f64 = before[size_start..].parse().expect("font size operand");
        let index: usize = before[..size_start].trim_end().rsplit("/F").next().expect("font resource name").parse().unwrap();
        uses.push((index, size));
        cursor = end + 3;
    }
    uses
}

fn resolved(text: &str, index: usize) -> String {
    let resources = font_resources(text);
    let objects = font_objects(text);
    let object = resources.iter().find(|(resource, _)| *resource == index).map(|(_, object)| *object).expect("resource");
    objects.iter().find(|(number, _)| *number == object).map(|(_, name)| name.clone()).expect("font object")
}

#[test]
fn bold_and_italic_runs_use_distinct_base14_fonts() {
    let text = render(vec![
        text("serif", FontStyle::Normal, "Regular"),
        text("serif", FontStyle::Bold, "Bold"),
        text("serif", FontStyle::Italic, "Italic"),
    ]);

    let names: Vec<String> = font_objects(&text).into_iter().map(|(_, name)| name).collect();
    assert_eq!(names, ["Times-Roman", "Times-Bold", "Times-Italic"], "canonical resource order");
    for name in ["Times-Roman", "Times-Bold", "Times-Italic"] {
        assert_eq!(text.matches(&format!("/BaseFont /{name}")).count(), 1, "`{name}` is deduplicated");
    }

    let uses = font_uses(&text);
    assert_eq!(uses.len(), 3, "one `Tf` per text run: {uses:?}");
    assert_eq!(resolved(&text, uses[0].0), "Times-Roman");
    assert_eq!(resolved(&text, uses[1].0), "Times-Bold");
    assert_eq!(resolved(&text, uses[2].0), "Times-Italic");
    assert_ne!(uses[0].0, uses[1].0, "bold must not reuse the regular resource");
    assert_ne!(uses[1].0, uses[2].0, "italic must not reuse the bold resource");
}

#[test]
fn family_and_style_resolve_to_the_base14_table() {
    let text = render(vec![
        text("sans-serif", FontStyle::Bold, "Sans bold"),
        text("monospace", FontStyle::Italic, "Mono italic"),
        text("serif", FontStyle::Normal, "Serif"),
        text("serif", FontStyle::Normal, "Serif again"),
    ]);

    let names: Vec<String> = font_objects(&text).into_iter().map(|(_, name)| name).collect();
    assert_eq!(names, ["Courier-Oblique", "Helvetica-Bold", "Times-Roman"], "canonical order, deduplicated");

    let uses = font_uses(&text);
    assert_eq!(uses.len(), 4);
    assert_eq!(resolved(&text, uses[0].0), "Helvetica-Bold");
    assert_eq!(resolved(&text, uses[1].0), "Courier-Oblique");
    assert_eq!(resolved(&text, uses[2].0), "Times-Roman");
    assert_eq!(uses[2].0, uses[3].0, "unstyled serif runs share one object");
    assert!(text.contains("(Sans bold) Tj") && text.contains("(Mono italic) Tj"), "text stays byte-exact");
}

#[test]
fn styled_fonts_keep_the_xref_table_intact() {
    let bytes = render_bytes(vec![
        text("serif", FontStyle::Bold, "Bold"),
        text("serif", FontStyle::Italic, "Italic"),
        text("sans-serif", FontStyle::Bold, "Sans"),
    ]);
    let startxref = {
        let marker = b"startxref\n";
        let at = find(&bytes, marker).expect("startxref keyword") + marker.len();
        let end = at + bytes[at..].iter().position(|byte| !byte.is_ascii_digit()).expect("startxref number");
        std::str::from_utf8(&bytes[at..end]).expect("startxref digits").parse::<usize>().expect("startxref offset")
    };
    assert!(bytes[startxref..].starts_with(b"xref"), "startxref must point at the xref keyword");

    let table = std::str::from_utf8(&bytes[startxref..]).expect("xref table is ascii");
    let mut lines = table.lines();
    assert_eq!(lines.next(), Some("xref"));
    let header = lines.next().expect("xref subsection header");
    let size: usize = header.split_whitespace().nth(1).expect("xref size").parse().unwrap();
    assert_eq!(lines.next(), Some("0000000000 65535 f "), "entry 0 is the free head");

    let object_count = bytes.windows(7).filter(|window| *window == b" 0 obj\n").count();
    for index in 1..size {
        let entry = lines.next().expect("xref entry");
        assert_eq!(entry.len(), 19, "every xref entry is 20 bytes including the line feed");
        let offset: usize = entry[..10].parse().expect("xref entry offset");
        let expected = format!("{index} 0 obj");
        assert!(bytes[offset..].starts_with(expected.as_bytes()), "object `{expected}` must start at its xref offset");
    }
    assert_eq!(size, object_count + 1, "/Size covers every object plus the free head");
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("trailer\n<< /Size"), "trailer stays after the xref table");
}
