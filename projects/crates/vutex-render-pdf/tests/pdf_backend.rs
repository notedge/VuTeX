//! PDF backend tests: file structure, xref integrity, and painted primitives.

use davinci_scene::{DisplayScene, NodeKind, SceneNode, ShapeStyle, TextAnchor, TextStyle};
use davinci_types::{Color, Point, Rect, Size, Transform};
use vutex_presentation::display::{
    DisplayGraphic, DisplayImage, DisplayItem, DisplayPage, DisplayPresentation, DisplayRect, DisplayRule, DisplayTextRun,
};
use vutex_render_pdf::{PdfFontPolicy, render_pdf, render_pdf_with_font_policy};
use vutex_types::math::FontStyle;

fn page(width: f64, height: f64, items: Vec<DisplayItem>) -> DisplayPage {
    DisplayPage { width, height, items }
}

fn text(x: f64, y: f64, size: f64, family: &str, value: &str) -> DisplayItem {
    linked_text(x, y, size, family, value, None)
}

fn linked_text(x: f64, y: f64, size: f64, family: &str, value: &str, href: Option<&str>) -> DisplayItem {
    titled_text(x, y, size, family, value, href, None)
}

fn titled_text(x: f64, y: f64, size: f64, family: &str, value: &str, href: Option<&str>, title: Option<&str>) -> DisplayItem {
    DisplayItem::TextRun(DisplayTextRun {
        x,
        y,
        size,
        width: value.chars().count() as f64 * size * 0.5,
        font_family: family.to_string(),
        style: FontStyle::Normal,
        text: value.to_string(),
        href: href.map(str::to_string),
        title: title.map(str::to_string),
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
fn substitutes_characters_outside_latin1_when_embedding_is_disabled() {
    let bytes = render_pdf_with_font_policy(
        &DisplayPresentation { pages: vec![page(595.0, 842.0, vec![text(72.0, 100.0, 12.0, "sans", "标题 Title")])] },
        &PdfFontPolicy::Base14Only,
    )
    .expect("pdf render");
    let text = lossy(&bytes);

    assert!(text.contains("/BaseFont /Helvetica"), "base-14 font is used");
    assert!(!text.contains("/Type0"), "no font program is embedded");
    assert!(text.contains("(?? Title) Tj"), "non-latin1 characters become question marks");
    assert!(!text.contains("标题"), "no raw utf-8 bytes in winansi content");
}

#[test]
fn embeds_a_subset_for_text_outside_winansi() {
    let presentation =
        DisplayPresentation { pages: vec![page(595.0, 842.0, vec![text(72.0, 100.0, 12.0, "sans", "标题 Title")])] };
    let Some(_) = vutex_render_pdf_font_available()
    else {
        return;
    };
    let bytes = render_pdf(&presentation).expect("pdf render");
    let text = lossy(&bytes);

    assert!(text.contains("/Subtype /Type0"), "composite font object");
    assert!(text.contains("/Encoding /Identity-H"), "identity cid mapping");
    assert!(text.contains("/Subtype /CIDFontType2"), "embedded TrueType program");
    assert!(text.contains("/FontFile2 "), "font file stream");
    assert!(text.contains("/ToUnicode "), "code point mapping for extraction");
    assert!(text.contains(" Tj"), "content stream still paints text");
    assert!(!text.contains("(?? Title) Tj"), "characters no longer degrade to question marks");

    let entries = xref_entries(&bytes);
    let numbers = object_numbers(&bytes);
    assert_eq!(entries.len(), numbers.len(), "one xref entry per object");
    assert_eq!(trailer_size(&bytes), numbers.into_iter().max().expect("at least one object") + 1);
}

/// Whether a system font for embedding is discoverable on this machine.
fn vutex_render_pdf_font_available() -> Option<()> {
    render_pdf_with_font_policy(
        &DisplayPresentation { pages: vec![page(595.0, 842.0, vec![text(72.0, 100.0, 12.0, "sans", "标题")])] },
        &PdfFontPolicy::Discover,
    )
    .ok()
    .filter(|bytes| lossy(bytes).contains("/FontFile2 "))
    .map(|_| ())
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

/// `/Rect [x1 y1 x2 y2]` of every link annotation, in document order.
fn link_rects(text: &str) -> Vec<[f64; 4]> {
    let needle = "/Subtype /Link /Rect [";
    let mut rects = Vec::new();
    let mut cursor = 0;
    while let Some(found) = text[cursor..].find(needle) {
        let start = cursor + found + needle.len();
        let end = start + text[start..].find(']').expect("link rect terminator");
        let values: Vec<f64> =
            text[start..end].split_whitespace().map(|value| value.parse().expect("link rect coordinate")).collect();
        assert_eq!(values.len(), 4, "a link rect carries four coordinates");
        rects.push([values[0], values[1], values[2], values[3]]);
        cursor = end;
    }
    rects
}

#[test]
fn safe_link_becomes_a_page_annotation() {
    let bytes = render_pdf(&DisplayPresentation {
        pages: vec![page(595.0, 842.0, vec![linked_text(72.0, 100.0, 12.0, "serif", "example", Some("https://example.com"))])],
    })
    .expect("pdf render");
    let text = lossy(&bytes);

    assert!(text.contains("/Subtype /Link"), "link annotation object");
    assert!(text.contains("/S /URI"), "uri action");
    assert!(text.contains("/URI (https://example.com)"), "destination literal string");
    assert!(text.contains("/Annots ["), "page references its annotations");

    let annots_at = text.find("/Annots [").expect("annots array") + "/Annots [".len();
    let number: usize = text[annots_at..].split_whitespace().next().expect("annotation reference").parse().unwrap();
    assert!(text.contains(&format!("{number} 0 obj\n<< /Type /Annot")), "reference points at the annotation object");

    let rects = link_rects(&text);
    assert_eq!(rects.len(), 1, "one annotation per safe run");
    let [x1, y1, x2, y2] = rects[0];
    assert!((x1 - 72.0).abs() < 1e-9, "rect starts at the run left edge");
    assert!((x2 - (72.0 + 42.0)).abs() < 1e-9, "rect spans the run width");
    assert!((y1 - (842.0 - (100.0 + 2.4))).abs() < 1e-9, "rect bottom sits 0.2 em below the baseline");
    assert!((y2 - (842.0 - (100.0 - 9.6))).abs() < 1e-9, "rect top sits 0.8 em above the baseline");
    assert!(x1 >= 0.0 && x2 <= 595.0 && y1 >= 0.0 && y2 <= 842.0, "rect stays inside the MediaBox");
}

#[test]
fn unsafe_link_emits_no_annotation() {
    let bytes = render_pdf(&DisplayPresentation {
        pages: vec![page(595.0, 842.0, vec![linked_text(72.0, 100.0, 12.0, "serif", "example", Some("javascript:alert(1)"))])],
    })
    .expect("pdf render");
    let text = lossy(&bytes);

    assert!(!text.contains("/Annots"), "no annotation array without a safe destination");
    assert!(!text.contains("/Subtype /Link"), "no link object");
    assert!(!text.contains("javascript:"), "unsafe destination never reaches the file");
    assert!(text.contains("(example) Tj"), "the text still renders");
}

#[test]
fn link_annotation_carries_the_authored_title() {
    let bytes = render_pdf(&DisplayPresentation {
        pages: vec![page(
            595.0,
            842.0,
            vec![titled_text(72.0, 100.0, 12.0, "serif", "example", Some("https://example.com"), Some("Spec (draft)"))],
        )],
    })
    .expect("pdf render");
    let text = lossy(&bytes);

    assert!(text.contains("/URI (https://example.com)"), "destination literal string");
    assert!(text.contains("/Contents (Spec \\(draft\\))"), "title literal string with escaped parentheses: {text}");
}

#[test]
fn link_annotation_omits_contents_without_a_title() {
    let bytes = render_pdf(&DisplayPresentation {
        pages: vec![page(595.0, 842.0, vec![linked_text(72.0, 100.0, 12.0, "serif", "example", Some("https://example.com"))])],
    })
    .expect("pdf render");
    let text = lossy(&bytes);

    assert!(text.contains("/Subtype /Link"), "the annotation still exists");
    assert!(!text.contains("/Contents ("), "no description entry without an authored title: {text}");
}

#[test]
fn pages_without_links_keep_no_annotations() {
    let bytes = render_pdf(&DisplayPresentation { pages: vec![full_page()] }).expect("pdf render");
    let text = lossy(&bytes);

    assert!(!text.contains("/Annots"), "link-free pages stay annotation-free");
    assert!(!text.contains("/Subtype /Link"));
    assert!(text.contains("(Hello PDF) Tj"), "existing content is unchanged");
}

#[test]
fn link_annotations_keep_the_xref_table_intact() {
    let bytes = render_pdf(&DisplayPresentation {
        pages: vec![
            page(595.0, 842.0, vec![linked_text(72.0, 100.0, 12.0, "serif", "first", Some("https://example.com"))]),
            page(612.0, 792.0, vec![linked_text(72.0, 100.0, 12.0, "sans", "second", Some("https://example.org"))]),
        ],
    })
    .expect("pdf render");

    let entries = xref_entries(&bytes);
    let numbers = object_numbers(&bytes);
    assert_eq!(entries.len(), numbers.len(), "one xref entry per object");
    for (index, offset) in entries.iter().enumerate() {
        let expected = format!("{} 0 obj", index + 1);
        assert!(bytes[*offset..].starts_with(expected.as_bytes()), "object {expected} at offset {offset}");
    }
    let highest = numbers.into_iter().max().expect("at least one object");
    assert_eq!(trailer_size(&bytes), highest + 1, "/Size is the highest object number plus one");
}

/// A scene with one shape of each family a diagram producer emits.
fn diagram_graphic() -> DisplayItem {
    let mut scene = DisplayScene::new(Size::new(200.0, 100.0));
    scene.push(SceneNode::new(NodeKind::RoundedRect {
        rect: Rect::new(10.0, 10.0, 80.0, 40.0),
        radius: 6.0,
        style: ShapeStyle::filled(Color::rgb(240, 240, 240)),
    }));
    scene.push(SceneNode::new(NodeKind::Line {
        from: Point::new(90.0, 30.0),
        to: Point::new(150.0, 60.0),
        style: ShapeStyle::stroked(Color::BLACK, 1.5),
    }));
    scene.push(SceneNode::new(NodeKind::Ellipse {
        center: Point::new(170.0, 70.0),
        radii: Size::new(12.0, 12.0),
        style: ShapeStyle::stroked(Color::BLACK, 1.0),
    }));
    let mut text = SceneNode::new(NodeKind::Text {
        position: Point::new(50.0, 36.0),
        text: "Start".to_string(),
        style: TextStyle { font_size: 12.0, text_anchor: TextAnchor::Middle, ..TextStyle::default() },
    });
    text.transform = Transform::new(1.0, 0.0, 0.0, 1.0, 0.0, 0.0);
    scene.push(text);
    DisplayItem::Graphic(DisplayGraphic { x: 72.0, y: 400.0, width: 400.0, height: 200.0, scene, alt: "diagram".to_string() })
}

#[test]
fn paints_a_diagram_scene_inside_its_box() {
    let bytes =
        render_pdf(&DisplayPresentation { pages: vec![page(595.0, 842.0, vec![diagram_graphic()])] }).expect("pdf render");
    let text = lossy(&bytes);

    assert!(text.contains("72 242 400 200 re\nW n"), "scene clips to the placed box: {text}");
    assert!(text.contains("2 0 0 -2 72 442 cm"), "scene space maps onto the box top edge with a y flip");
    assert!(text.contains("h\n"), "closed subpaths: {text}");
    assert!(text.contains(" c\n"), "corners and ellipses are cubic Béziers");
    assert!(text.contains("1.5 w\n"), "stroke width reaches the content stream");
    assert!(text.contains("(Start) Tj"), "label text is painted");
    assert!(text.contains("1 0 0 -1 32 36 Tm"), "anchored label stays upright under the flipped matrix");
}

#[test]
fn skips_a_scene_without_a_canvas() {
    let mut scene = DisplayScene::new(Size::new(0.0, 0.0));
    scene.push(SceneNode::new(NodeKind::Rect {
        rect: Rect::new(0.0, 0.0, 10.0, 10.0),
        style: ShapeStyle::filled(Color::BLACK),
    }));
    let graphic =
        DisplayItem::Graphic(DisplayGraphic { x: 0.0, y: 0.0, width: 10.0, height: 10.0, scene, alt: "empty".to_string() });
    let bytes = render_pdf(&DisplayPresentation { pages: vec![page(595.0, 842.0, vec![graphic])] }).expect("pdf render");
    let text = lossy(&bytes);

    assert!(!text.contains(" cm\n"), "a zero-sized canvas paints nothing: {text}");
}

#[test]
fn embeds_scene_labels_outside_winansi() {
    let Some(_) = vutex_render_pdf_font_available()
    else {
        return;
    };
    let mut scene = DisplayScene::new(Size::new(100.0, 50.0));
    scene.push(SceneNode::new(NodeKind::Text {
        position: Point::new(10.0, 30.0),
        text: "开始".to_string(),
        style: TextStyle { font_size: 12.0, ..TextStyle::default() },
    }));
    let graphic =
        DisplayItem::Graphic(DisplayGraphic {
            x: 72.0, y: 100.0, width: 200.0, height: 100.0, scene, alt: "节点".to_string()
        });
    let bytes = render_pdf(&DisplayPresentation { pages: vec![page(595.0, 842.0, vec![graphic])] }).expect("pdf render");
    let text = lossy(&bytes);

    assert!(text.contains("/FontFile2 "), "scene label pulls in the embedded subset");
    assert!(!text.contains("(??) Tj"), "scene labels never degrade to question marks");
}
