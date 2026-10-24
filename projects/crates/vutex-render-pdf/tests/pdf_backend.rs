//! PDF backend tests: file structure, xref integrity, and painted primitives.

use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use davinci_scene::{DisplayScene, NodeKind, ResourceRef, SceneNode, ShapeStyle, TextAnchor, TextStyle};
use davinci_types::{Color, Point, Rect, Size, Transform};
use vutex_presentation::display::{
    DisplayGraphic, DisplayImage, DisplayItem, DisplayPage, DisplayPresentation, DisplayRect, DisplayRule, DisplayTextRun,
};
use vutex_render_pdf::{PdfFontPolicy, PdfResources, render_pdf, render_pdf_with_font_policy, render_pdf_with_resources};
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
            DisplayItem::Image(DisplayImage {
                x: 72.0,
                y: 260.0,
                width: 120.0,
                height: 60.0,
                alt: "figure".to_string(),
                source: None,
            }),
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

/// Assert the xref table and trailer describe exactly the appended objects.
fn assert_xref_intact(bytes: &[u8]) {
    let entries = xref_entries(bytes);
    let numbers = object_numbers(bytes);
    assert_eq!(entries.len(), numbers.len(), "one xref entry per object");
    for (index, offset) in entries.iter().enumerate() {
        let expected = format!("{} 0 obj", index + 1);
        assert!(bytes[*offset..].starts_with(expected.as_bytes()), "object {expected} at offset {offset}");
    }
    let highest = numbers.into_iter().max().expect("at least one object");
    assert_eq!(trailer_size(bytes), highest + 1, "/Size is the highest object number plus one");
}

/// Whether every `q` operator is closed by a later `Q` in the same nesting.
fn balanced_graphics_states(text: &str) -> bool {
    let mut depth: i64 = 0;
    for line in text.lines() {
        match line.trim() {
            "q" => depth += 1,
            "Q" => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            _ => {}
        }
    }
    depth == 0
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

/// Scene item carrying the nodes a test builds on a 200 x 100 canvas.
fn scene_graphic(scene: DisplayScene) -> DisplayItem {
    DisplayItem::Graphic(DisplayGraphic { x: 72.0, y: 400.0, width: 400.0, height: 200.0, scene, alt: "faded".to_string() })
}

/// A translucent rectangle and a translucent label in one scene.
fn translucent_graphic() -> DisplayItem {
    let mut scene = DisplayScene::new(Size::new(200.0, 100.0));
    scene.push(SceneNode::new(NodeKind::Rect {
        rect: Rect::new(10.0, 10.0, 80.0, 40.0),
        style: ShapeStyle { opacity: 0.5, ..ShapeStyle::filled(Color::BLACK) },
    }));
    scene.push(SceneNode::new(NodeKind::Text {
        position: Point::new(20.0, 36.0),
        text: "Faded".to_string(),
        style: TextStyle { font_size: 12.0, opacity: 0.75, ..TextStyle::default() },
    }));
    scene_graphic(scene)
}

#[test]
fn paints_scene_opacity_through_ext_gstates() {
    let bytes =
        render_pdf(&DisplayPresentation { pages: vec![page(595.0, 842.0, vec![translucent_graphic()])] }).expect("pdf render");
    let text = lossy(&bytes);

    assert_eq!(text.matches("/Type /ExtGState").count(), 2, "one state per distinct alpha: {text}");
    assert!(text.contains("/ExtGState << /GS1 "), "resource category names the states: {text}");
    assert!(
        text.contains("<< /Type /ExtGState /ca 0.5 /CA 0.5 /AIS false /BM /Normal >>"),
        "fill and stroke alpha share the shape value: {text}"
    );
    assert!(text.contains("/ca 0.75"), "the label keeps its own alpha");
    assert_eq!(text.matches("/GS1 gs\n").count(), 1, "the rectangle selects the 0.5 state");
    assert_eq!(text.matches("/GS2 gs\n").count(), 1, "the label selects the 0.75 state");
    assert!(balanced_graphics_states(&text), "every `q` is closed by its `Q`");
    assert_xref_intact(&bytes);
}

#[test]
fn scenes_sharing_an_opacity_share_one_ext_gstate() {
    let mut scene = DisplayScene::new(Size::new(100.0, 100.0));
    scene.push(SceneNode::new(NodeKind::Rect {
        rect: Rect::new(0.0, 0.0, 40.0, 40.0),
        style: ShapeStyle { opacity: 0.35, ..ShapeStyle::stroked(Color::BLACK, 1.0) },
    }));
    scene.push(SceneNode::new(NodeKind::Ellipse {
        center: Point::new(70.0, 70.0),
        radii: Size::new(20.0, 20.0),
        style: ShapeStyle { opacity: 0.35, ..ShapeStyle::filled(Color::rgb(0, 0, 255)) },
    }));
    let bytes =
        render_pdf(&DisplayPresentation { pages: vec![page(595.0, 842.0, vec![scene_graphic(scene)])] }).expect("pdf render");
    let text = lossy(&bytes);

    assert_eq!(text.matches("/Type /ExtGState").count(), 1, "one object for the shared alpha: {text}");
    assert!(text.contains("/ca 0.35"), "the shared value reaches the dictionary");
    assert_eq!(text.matches("/GS1 gs\n").count(), 2, "both nodes select the same state");
    assert_xref_intact(&bytes);
}

#[test]
fn fully_opaque_scenes_emit_no_graphics_state() {
    let bytes =
        render_pdf(&DisplayPresentation { pages: vec![page(595.0, 842.0, vec![diagram_graphic()])] }).expect("pdf render");
    let text = lossy(&bytes);

    assert!(!text.contains("/ExtGState"), "opaque scenes need no alpha resource: {text}");
    assert!(!text.contains(" gs\n"), "no `gs` operator appears");
    assert!(balanced_graphics_states(&text), "existing state pairs stay balanced");
    assert_xref_intact(&bytes);
}

#[test]
fn group_opacity_multiplies_into_descendants() {
    let mut children = vec![SceneNode::new(NodeKind::Rect {
        rect: Rect::new(0.0, 0.0, 20.0, 20.0),
        style: ShapeStyle { opacity: 0.5, ..ShapeStyle::filled(Color::BLACK) },
    })];
    children.push(SceneNode::new(NodeKind::Rect {
        rect: Rect::new(60.0, 60.0, 20.0, 20.0),
        style: ShapeStyle::filled(Color::BLACK),
    }));
    let mut scene = DisplayScene::new(Size::new(100.0, 100.0));
    scene.push(SceneNode::new(NodeKind::Group { children, opacity: 0.5 }));
    let bytes =
        render_pdf(&DisplayPresentation { pages: vec![page(595.0, 842.0, vec![scene_graphic(scene)])] }).expect("pdf render");
    let text = lossy(&bytes);

    assert!(text.contains("/ca 0.25"), "group alpha composes with the child alpha: {text}");
    assert!(text.contains("/ca 0.5"), "a child without its own opacity inherits the group alpha");
    assert!(!text.contains("<< /Type /ExtGState /ca 1 "), "the group never paints one flattened state");
    assert_eq!(text.matches("/Type /ExtGState").count(), 2, "one state per composed alpha");
    assert_eq!(text.matches("/GS1 gs\n").count(), 1, "the translucent child selects the 0.25 state");
    assert_eq!(text.matches("/GS2 gs\n").count(), 1, "the opaque child still inherits the 0.5 state");
    assert!(balanced_graphics_states(&text));
    assert_xref_intact(&bytes);
}

#[test]
fn malformed_opacity_is_clamped_or_ignored() {
    let mut scene = DisplayScene::new(Size::new(100.0, 50.0));
    scene.push(SceneNode::new(NodeKind::Rect {
        rect: Rect::new(0.0, 0.0, 10.0, 10.0),
        style: ShapeStyle { opacity: f64::NAN, ..ShapeStyle::filled(Color::BLACK) },
    }));
    scene.push(SceneNode::new(NodeKind::Rect {
        rect: Rect::new(20.0, 0.0, 10.0, 10.0),
        style: ShapeStyle { opacity: 1.4, ..ShapeStyle::filled(Color::BLACK) },
    }));
    scene.push(SceneNode::new(NodeKind::Rect {
        rect: Rect::new(40.0, 0.0, 10.0, 10.0),
        style: ShapeStyle { opacity: -0.5, ..ShapeStyle::filled(Color::BLACK) },
    }));
    let bytes =
        render_pdf(&DisplayPresentation { pages: vec![page(595.0, 842.0, vec![scene_graphic(scene)])] }).expect("pdf render");
    let text = lossy(&bytes);

    assert!(!text.contains("/ca nan") && !text.contains("/ca inf"), "non-finite alpha stays out of the file: {text}");
    assert!(!text.contains("/ca 1.4"), "out-of-range alpha is clamped, never written raw");
    assert!(text.contains("/ca 0 /CA 0 "), "a negative alpha clamps to fully transparent: {text}");
    assert_xref_intact(&bytes);
}

/// PNG file signature, as authored by the test fixture builder.
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// Scratch image file removed when its test ends.
struct ScratchImage {
    path: PathBuf,
}

impl ScratchImage {
    /// Write `bytes` under a unique name in the system temp directory.
    fn write(name: &str, bytes: &[u8]) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let sequence = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("vutex-pdf-image-{}-{sequence}-{name}", std::process::id()));
        std::fs::write(&path, bytes).expect("write scratch image");
        Self { path }
    }

    /// Path a scene resource can reference.
    fn href(&self) -> String {
        self.path.to_string_lossy().into_owned()
    }
}

impl Drop for ScratchImage {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Scratch directory holding images a relative source can resolve against.
struct ScratchDir {
    path: PathBuf,
}

impl ScratchDir {
    /// Create a unique directory in the system temp directory.
    fn new(name: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let sequence = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("vutex-pdf-dir-{}-{sequence}-{name}", std::process::id()));
        std::fs::create_dir_all(&path).expect("create scratch directory");
        Self { path }
    }

    /// Write `bytes` under `name` inside the directory.
    fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.path.join(name);
        std::fs::write(&path, bytes).expect("write scratch image");
        path
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// Append one PNG chunk with a zero CRC placeholder.
///
/// The writer's decoder does not verify chunk checksums, so the fixture stays
/// deterministic without carrying a CRC implementation.
fn png_chunk(out: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
    out.extend_from_slice(&[0; 4]);
}

/// Wrap filtered scanlines in a PNG whose single `IDAT` is one stored DEFLATE
/// block.
fn png_fixture(
    width: u32,
    height: u32,
    bit_depth: u8,
    color_type: u8,
    interlace: u8,
    transparency: Option<&[u8]>,
    scanlines: &[u8],
) -> Vec<u8> {
    let length = scanlines.len() as u16;
    // zlib header, one final stored block (`LEN`, `NLEN` = !`LEN`), the
    // filtered samples, and an Adler-32 trailer the decoder ignores.
    let mut zlib = vec![0x78, 0x01, 0x01];
    zlib.extend_from_slice(&length.to_le_bytes());
    zlib.extend_from_slice(&(!length).to_le_bytes());
    zlib.extend_from_slice(scanlines);
    zlib.extend_from_slice(&[0; 4]);

    let mut header = Vec::new();
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    header.extend_from_slice(&[bit_depth, color_type, 0, 0, interlace]);
    let mut png = PNG_SIGNATURE.to_vec();
    png_chunk(&mut png, b"IHDR", &header);
    if let Some(body) = transparency {
        png_chunk(&mut png, b"tRNS", body);
    }
    png_chunk(&mut png, b"IDAT", &zlib);
    png_chunk(&mut png, b"IEND", &[]);
    png
}

/// Wrap filtered scanlines in a 2×2 PNG of the given colour type whose single
/// `IDAT` is one stored DEFLATE block.
fn png_with_color_type(color_type: u8, scanlines: &[u8]) -> Vec<u8> {
    png_fixture(2, 2, 8, color_type, 0, None, scanlines)
}

/// Wrap filtered scanlines in a 2×2 RGB PNG whose single `IDAT` is one stored
/// DEFLATE block.
fn png_from_scanlines(scanlines: &[u8]) -> Vec<u8> {
    png_with_color_type(2, scanlines)
}

/// Unfiltered 2×2 truecolour scanlines: red and green, then blue and white.
const TINY_RGB_SCANLINES: [u8; 14] = [
    0, 255, 0, 0, 0, 255, 0, // red, green
    0, 0, 0, 255, 255, 255, 255, // blue, white
];

/// A 2×2 RGB PNG whose rows carry no filter.
fn tiny_png() -> Vec<u8> {
    png_from_scanlines(&TINY_RGB_SCANLINES)
}

/// A 2×2 truecolour PNG whose `tRNS` body names the given colour key.
fn keyed_truecolour_png(key: &[u8]) -> Vec<u8> {
    png_fixture(2, 2, 8, 2, 0, Some(key), &TINY_RGB_SCANLINES)
}

/// Filtered 16-bit truecolour scanlines: the high bytes spell `12 56 9A`,
/// `FF 00 80`, `01 03 05`, and `AB EF 23`.
const SIXTEEN_BIT_RGB_SCANLINES: [u8; 26] = [
    0, // filter none
    0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, // first pixel
    0xFF, 0x00, 0x00, 0xFF, 0x80, 0x00, // second pixel
    0,    // filter none
    0x01, 0x02, 0x03, 0x04, 0x05, 0x06, //
    0xAB, 0xCD, 0xEF, 0x01, 0x23, 0x45, //
];

/// A 2×2 truecolour PNG with 16-bit samples.
fn sixteen_bit_png() -> Vec<u8> {
    png_fixture(2, 2, 16, 2, 0, None, &SIXTEEN_BIT_RGB_SCANLINES)
}

/// A 2×2 RGBA PNG with 16-bit colour and alpha samples.
fn sixteen_bit_alpha_png() -> Vec<u8> {
    png_fixture(
        2,
        2,
        16,
        6,
        0,
        None,
        &[
            0, // filter none
            0xFF, 0xFF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // red, alpha 0
            0x00, 0x00, 0xFF, 0xFF, 0x00, 0x00, 0x40, 0x00, // green, alpha 64
            0,    // filter none
            0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0x80, 0x00, // blue, alpha 128
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, // white, alpha 255
        ],
    )
}

/// A 2×2 greyscale PNG whose samples are 10, 20, 30, and 20 with a colour key.
fn keyed_grey_png(key: &[u8]) -> Vec<u8> {
    png_fixture(2, 2, 8, 0, 0, Some(key), &[0, 10, 20, 0, 30, 20])
}

/// A 2×2 RGBA PNG with the alpha samples 0, 64, 128, and 255.
///
/// The colour plane is red, green, blue, white, so it stays distinguishable
/// from the alpha plane in the emitted file.
fn alpha_png() -> Vec<u8> {
    png_with_color_type(
        6,
        &[
            0, 255, 0, 0, 0, 0, 255, 0, 64, // red with alpha 0, green with alpha 64
            0, 0, 0, 255, 128, 255, 255, 255, 255, // blue with alpha 128, white
        ],
    )
}

/// A 2×2 grey-alpha PNG whose alpha samples are 0 and 255.
fn grey_alpha_png() -> Vec<u8> {
    png_with_color_type(
        4,
        &[
            0, 10, 0, 20, 255, // grey 10 transparent, grey 20 opaque
            0, 10, 0, 20, 255,
        ],
    )
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

/// A scene that places one image node per `(href, mime)` pair.
fn scene_with_images(images: &[(&str, Option<&str>)]) -> DisplayItem {
    let mut scene = DisplayScene::new(Size::new(100.0, 50.0));
    for (index, (href, mime)) in images.iter().enumerate() {
        let mut resource = ResourceRef::new(*href);
        if let Some(mime) = mime {
            resource = resource.with_mime(*mime);
        }
        scene.push(SceneNode::new(NodeKind::Image {
            rect: Rect::new(10.0, 10.0 + 20.0 * index as f64, 20.0, 10.0),
            resource,
            opacity: 1.0,
        }));
    }
    DisplayItem::Graphic(DisplayGraphic { x: 72.0, y: 100.0, width: 200.0, height: 100.0, scene, alt: "图片".to_string() })
}

/// Render one page that carries the given scene item.
fn render_scene(item: DisplayItem) -> Vec<u8> {
    render_pdf(&DisplayPresentation { pages: vec![page(595.0, 842.0, vec![item])] }).expect("pdf render")
}

/// Render one scene item through an explicit resource context.
fn render_scene_with(item: DisplayItem, resources: &PdfResources) -> Vec<u8> {
    render_pdf_with_resources(
        &DisplayPresentation { pages: vec![page(595.0, 842.0, vec![item])] },
        &PdfFontPolicy::Discover,
        resources,
    )
    .expect("pdf render")
}

/// A minimal baseline JPEG: SOI, `APP0`, `DQT`, `SOF0` for a 2×2 RGB frame,
/// `DHT`, `SOS`, and `EOI`, with two bytes of scan data.
const JPEG: &[u8] = &[
    0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0x00, 0x01, 0x01, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00,
    0xFF, 0xDB, 0x00, 0x43, 0x00, 0x03, 0x02, 0x02, 0x03, 0x02, 0x02, 0x03, 0x03, 0x03, 0x03, 0x04, 0x03, 0x03, 0x04, 0x05,
    0x08, 0x05, 0x05, 0x04, 0x04, 0x05, 0x0A, 0x07, 0x07, 0x06, 0x08, 0x0C, 0x0A, 0x0C, 0x0C, 0x0B, 0x0A, 0x0B, 0x0B, 0x0D,
    0x0E, 0x12, 0x10, 0x0D, 0x0E, 0x11, 0x0E, 0x0B, 0x0B, 0x10, 0x16, 0x10, 0x11, 0x13, 0x14, 0x15, 0x15, 0x15, 0x0C, 0x0F,
    0x17, 0x18, 0x16, 0x14, 0x18, 0x12, 0x14, 0x15, 0x14, 0xFF, 0xDB, 0x00, 0x43, 0x01, 0x03, 0x04, 0x04, 0x05, 0x04, 0x05,
    0x09, 0x05, 0x05, 0x09, 0x14, 0x0D, 0x0B, 0x0D, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14,
    0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14,
    0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0xFF, 0xC0,
    0x00, 0x11, 0x08, 0x00, 0x02, 0x00, 0x02, 0x03, 0x01, 0x11, 0x00, 0x02, 0x11, 0x01, 0x03, 0x11, 0x01, 0xFF, 0xC4, 0x00,
    0x1F, 0x00, 0x00, 0x01, 0x05, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01,
    0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0xFF, 0xC4, 0x00, 0xB5, 0x10, 0x00, 0x02, 0x01, 0x03, 0x03,
    0x02, 0x04, 0x03, 0x05, 0x05, 0x04, 0x04, 0x00, 0x00, 0x01, 0x7D, 0x01, 0x02, 0x03, 0x00, 0x04, 0x11, 0x05, 0x12, 0x21,
    0x31, 0x41, 0x06, 0x13, 0x51, 0x61, 0x07, 0x22, 0x71, 0x14, 0x32, 0x81, 0x91, 0xA1, 0x08, 0x23, 0x42, 0xB1, 0xC1, 0x15,
    0x52, 0xD1, 0xF0, 0x24, 0x33, 0x62, 0x72, 0x82, 0x09, 0x0A, 0x16, 0x17, 0x18, 0x19, 0x1A, 0x25, 0x26, 0x27, 0x28, 0x29,
    0x2A, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3A, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49, 0x4A, 0x53, 0x54, 0x55, 0x56,
    0x57, 0x58, 0x59, 0x5A, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x6A, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7A,
    0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8A, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9A, 0xA2, 0xA3, 0xA4,
    0xA5, 0xA6, 0xA7, 0xA8, 0xA9, 0xAA, 0xB2, 0xB3, 0xB4, 0xB5, 0xB6, 0xB7, 0xB8, 0xB9, 0xBA, 0xC2, 0xC3, 0xC4, 0xC5, 0xC6,
    0xC7, 0xC8, 0xC9, 0xCA, 0xD2, 0xD3, 0xD4, 0xD5, 0xD6, 0xD7, 0xD8, 0xD9, 0xDA, 0xE1, 0xE2, 0xE3, 0xE4, 0xE5, 0xE6, 0xE7,
    0xE8, 0xE9, 0xEA, 0xF1, 0xF2, 0xF3, 0xF4, 0xF5, 0xF6, 0xF7, 0xF8, 0xF9, 0xFA, 0xFF, 0xC4, 0x00, 0x1F, 0x01, 0x00, 0x03,
    0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x02, 0x03, 0x04, 0x05,
    0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0xFF, 0xC4, 0x00, 0xB5, 0x11, 0x00, 0x02, 0x01, 0x02, 0x04, 0x04, 0x03, 0x04, 0x07,
    0x05, 0x04, 0x04, 0x00, 0x01, 0x02, 0x77, 0x00, 0x01, 0x02, 0x03, 0x11, 0x04, 0x05, 0x21, 0x31, 0x06, 0x12, 0x41, 0x51,
    0x07, 0x61, 0x71, 0x13, 0x22, 0x32, 0x81, 0x08, 0x14, 0x42, 0x91, 0xA1, 0xB1, 0xC1, 0x09, 0x23, 0x33, 0x52, 0xF0, 0x15,
    0x62, 0x72, 0xD1, 0x0A, 0x16, 0x24, 0x34, 0xE1, 0x25, 0xF1, 0x17, 0x18, 0x19, 0x1A, 0x26, 0x27, 0x28, 0x29, 0x2A, 0x35,
    0x36, 0x37, 0x38, 0x39, 0x3A, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49, 0x4A, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59,
    0x5A, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x6A, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7A, 0x82, 0x83, 0x84,
    0x85, 0x86, 0x87, 0x88, 0x89, 0x8A, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9A, 0xA2, 0xA3, 0xA4, 0xA5, 0xA6,
    0xA7, 0xA8, 0xA9, 0xAA, 0xB2, 0xB3, 0xB4, 0xB5, 0xB6, 0xB7, 0xB8, 0xB9, 0xBA, 0xC2, 0xC3, 0xC4, 0xC5, 0xC6, 0xC7, 0xC8,
    0xC9, 0xCA, 0xD2, 0xD3, 0xD4, 0xD5, 0xD6, 0xD7, 0xD8, 0xD9, 0xDA, 0xE2, 0xE3, 0xE4, 0xE5, 0xE6, 0xE7, 0xE8, 0xE9, 0xEA,
    0xF2, 0xF3, 0xF4, 0xF5, 0xF6, 0xF7, 0xF8, 0xF9, 0xFA, 0xFF, 0xDA, 0x00, 0x0C, 0x03, 0x01, 0x00, 0x02, 0x11, 0x03, 0x11,
    0x00, 0x3F, 0x00, 0xFB, 0xAF, 0xF6, 0x6F, 0xF0, 0x1F, 0x86, 0xB5, 0x2F, 0xD9, 0xE3, 0xE1, 0x75, 0xDD, 0xDF, 0x87, 0x74,
    0x9B, 0xAB, 0xBB, 0x8F, 0x0B, 0x69, 0x72, 0xCD, 0x3C, 0xF6, 0x31, 0x3C, 0x92, 0x3B, 0x5A, 0x44, 0x59, 0x99, 0x8A, 0xE4,
    0x92, 0x49, 0x24, 0x9E, 0xB9, 0xAE, 0xBE, 0x28, 0xE1, 0xEC, 0x9A, 0x96, 0x7D, 0x8F, 0xA7, 0x4F, 0x07, 0x49, 0x45, 0x56,
    0xAA, 0x92, 0x54, 0xE0, 0x92, 0x4A, 0x72, 0xB2, 0x4A, 0xDB, 0x1F, 0x3F, 0x53, 0x25, 0xCA, 0xF1, 0xF3, 0x78, 0xCC, 0x66,
    0x16, 0x9D, 0x4A, 0xB5, 0x1F, 0x34, 0xA5, 0x28, 0x46, 0x52, 0x94, 0xA5, 0xAC, 0xA5, 0x29, 0x34, 0xDB, 0x93, 0x6D, 0xB6,
    0xDB, 0x6D, 0xB7, 0x76, 0x7F, 0xFF, 0xD9,
];

#[test]
fn embeds_a_png_scene_image_as_a_raster_xobject() {
    let href = format!("data:image/png;base64,{}", base64(&tiny_png()));
    let bytes = render_scene(scene_with_images(&[(&href, None)]));
    let text = lossy(&bytes);

    assert_eq!(text.matches("/Type /XObject /Subtype /Image").count(), 1, "one image object: {text}");
    assert!(text.contains("/Width 2"), "PNG width reaches the dictionary: {text}");
    assert!(text.contains("/Height 2"), "PNG height reaches the dictionary: {text}");
    assert!(text.contains("/ColorSpace /DeviceRGB"), "truecolour PNG stays RGB: {text}");
    assert!(!text.contains("/SMask"), "an opaque PNG invents no soft mask: {text}");
    assert!(text.contains("/Length 12 >>"), "two rows of two RGB pixels are written unfiltered");
    assert!(!text.contains("/Filter"), "PNG samples need no stream filter: {text}");
    assert!(text.contains("/XObject << /Im1 "), "page resources register the image: {text}");
    assert!(text.contains("\n/Im1 Do\n"), "the content stream paints the image: {text}");
    assert!(!text.contains("[3 2] 0 d"), "a resolved image replaces the dashed slot");
    assert_image_orientation(&text);
    assert_xref_intact(&bytes);
}

#[test]
fn embeds_a_jpeg_scene_image_through_dct_decode() {
    let file = ScratchImage::write("tiny.jpg", JPEG);
    let href = file.href();
    let bytes = render_scene(scene_with_images(&[(&href, None)]));
    let text = lossy(&bytes);

    assert!(text.contains("/Filter /DCTDecode"), "JPEG keeps its compressed stream: {text}");
    assert!(text.contains("/Width 2"), "SOF width reaches the dictionary: {text}");
    assert!(text.contains("/Height 2"), "SOF height reaches the dictionary: {text}");
    assert!(text.contains("/ColorSpace /DeviceRGB"), "three components mean RGB: {text}");
    assert!(text.contains("\n/Im1 Do\n"), "the content stream paints the image: {text}");
    assert!(!text.contains("[3 2] 0 d"));
    assert_image_orientation(&text);
    assert_xref_intact(&bytes);
}

#[test]
fn unresolved_scene_images_keep_the_dashed_placeholder() {
    let missing = std::env::temp_dir().join(format!("vutex-pdf-absent-{}.png", std::process::id()));
    let _ = std::fs::remove_file(&missing);
    let missing = missing.to_string_lossy().into_owned();
    let gif = format!("data:image/gif;base64,{}", base64(b"GIF89a\x01\x00\x01\x00\x80\x00\x00"));
    let bytes = render_scene(scene_with_images(&[(&missing, None), (&gif, Some("image/gif"))]));
    let text = lossy(&bytes);

    assert_eq!(text.matches("[3 2] 0 d").count(), 2, "both slots fall back to the dashed box: {text}");
    assert!(!text.contains("/Subtype /Image"), "no object is invented for an unresolved reference");
    assert_xref_intact(&bytes);
}

#[test]
fn rebuilds_paeth_filtered_png_rows() {
    // Row zero is unfiltered. Row one uses the Paeth predictor, so its bytes
    // are deltas against the row above and only a correct reconstruction
    // yields the authored blue and white pixels.
    let png = png_from_scanlines(&[
        0, 255, 0, 0, 0, 255, 0, // filter none: red, green
        4, 1, 0, 255, 255, 0, 0, // filter paeth: blue, white
    ]);
    let href = format!("data:image/png;base64,{}", base64(&png));
    let bytes = render_scene(scene_with_images(&[(&href, None)]));

    let samples = [255u8, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255];
    assert!(
        bytes.windows(samples.len()).any(|window| window == samples),
        "reconstructed rows must be red, green, blue, white in that order"
    );
}

#[test]
fn repeated_image_references_share_one_xobject() {
    let href = format!("data:image/png;base64,{}", base64(&tiny_png()));
    let bytes = render_scene(scene_with_images(&[(&href, None), (&href, None)]));
    let text = lossy(&bytes);

    assert_eq!(text.matches("/Type /XObject /Subtype /Image").count(), 1, "one image object: {text}");
    assert_eq!(text.matches("\n/Im1 Do\n").count(), 2, "both placements select the same object");
    assert!(!text.contains("/Im2"), "the repeated reference gets no second name");
    assert!(text.contains("/XObject << /Im1 "));
    assert_xref_intact(&bytes);
}

/// Object number the colour image references as its `/SMask`.
fn mask_number(text: &str) -> usize {
    let at = text.find("/SMask ").expect("/SMask reference") + "/SMask ".len();
    let rest = &text[at..];
    let number: usize = rest.split_whitespace().next().expect("mask object number").parse().expect("numeric mask reference");
    assert!(rest.starts_with(&format!("{number} 0 R")), "/SMask carries an indirect reference: {rest}");
    number
}

/// Mask stream body located through the xref, so binary samples never pass
/// through UTF-8 decoding.
fn mask_stream(bytes: &[u8], number: usize) -> &[u8] {
    let offset = xref_entries(bytes)[number - 1];
    let object = &bytes[offset..];
    assert!(object.starts_with(format!("{number} 0 obj\n").as_bytes()), "xref points at the mask object");
    let marker = b"stream\n";
    let start = find(object, marker).expect("mask stream keyword") + marker.len();
    let length = find(&object[start..], b"\nendstream").expect("mask stream terminator");
    &object[start..start + length]
}

/// Colour stream body of the first image XObject, located without UTF-8
/// decoding.
fn image_stream(bytes: &[u8]) -> &[u8] {
    let at = find(bytes, b"/Subtype /Image").expect("image dictionary");
    let marker = b"stream\n";
    let start = find(&bytes[at..], marker).expect("image stream keyword") + at + marker.len();
    let length = find(&bytes[start..], b"\nendstream").expect("image stream terminator");
    &bytes[start..start + length]
}

#[test]
fn png_alpha_becomes_a_device_gray_soft_mask() {
    let href = format!("data:image/png;base64,{}", base64(&alpha_png()));
    let bytes = render_scene(scene_with_images(&[(&href, None)]));
    let text = lossy(&bytes);

    assert_eq!(text.matches("/Type /XObject /Subtype /Image").count(), 2, "a colour image and its mask: {text}");
    assert!(text.contains("/ColorSpace /DeviceRGB"), "the colour plane stays RGB: {text}");
    assert!(
        text.contains(
            "<< /Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace /DeviceGray /BitsPerComponent 8 /Length 4 >>"
        ),
        "the mask is a single-channel 8-bit image: {text}"
    );
    assert!(text.contains("/Length 12 >>"), "the colour plane keeps two rows of RGB samples");
    assert!(!text.contains("/Filter"), "neither plane needs a stream filter");

    let mask = mask_number(&text);
    assert!(text.contains(&format!("/SMask {mask} 0 R")), "the colour dictionary points at its mask: {text}");
    assert_eq!(mask_stream(&bytes, mask), [0, 64, 128, 255], "the mask carries the authored alpha samples");

    let color: [u8; 12] = [255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255];
    assert!(
        bytes.windows(color.len()).any(|window| window == color),
        "the colour stream keeps the red, green, blue, white samples"
    );
    assert_xref_intact(&bytes);
}

#[test]
fn sixteen_bit_png_narrows_to_its_high_bytes() {
    let href = format!("data:image/png;base64,{}", base64(&sixteen_bit_png()));
    let bytes = render_scene(scene_with_images(&[(&href, None)]));
    let text = lossy(&bytes);

    assert_eq!(text.matches("/Type /XObject /Subtype /Image").count(), 1, "one image object: {text}");
    assert!(text.contains("/Width 2") && text.contains("/Height 2"), "the frame reaches the dictionary: {text}");
    assert!(text.contains("/ColorSpace /DeviceRGB"), "truecolour stays RGB: {text}");
    assert!(text.contains("/BitsPerComponent 8"), "16-bit samples embed as 8-bit components: {text}");
    assert!(!text.contains("/SMask"), "an opaque 16-bit PNG invents no soft mask: {text}");
    assert!(text.contains("/Length 12 >>"), "narrowing turns two rows of 16-bit samples into twelve bytes: {text}");
    assert_eq!(
        image_stream(&bytes),
        [0x12, 0x56, 0x9A, 0xFF, 0x00, 0x80, 0x01, 0x03, 0x05, 0xAB, 0xEF, 0x23],
        "every sample keeps its high byte"
    );
    assert_xref_intact(&bytes);
}

#[test]
fn sixteen_bit_alpha_png_keeps_both_planes() {
    let href = format!("data:image/png;base64,{}", base64(&sixteen_bit_alpha_png()));
    let bytes = render_scene(scene_with_images(&[(&href, None)]));
    let text = lossy(&bytes);

    assert_eq!(text.matches("/Type /XObject /Subtype /Image").count(), 2, "one colour image and one mask: {text}");
    assert_eq!(text.matches("/BitsPerComponent 8").count(), 2, "both planes embed as 8-bit components: {text}");
    let mask = mask_number(&text);
    assert!(text.contains(&format!("/SMask {mask} 0 R")), "the colour image references the mask: {text}");
    assert_eq!(mask_stream(&bytes, mask), [0, 64, 128, 255], "the alpha plane narrows the same way");

    let color: [u8; 12] = [255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255];
    assert!(bytes.windows(color.len()).any(|window| window == color), "the colour plane keeps the high bytes");
    assert_xref_intact(&bytes);
}

#[test]
fn truecolour_colour_key_becomes_a_soft_mask() {
    // The key names green: red is `(0, 255, 0)` in the second pixel.
    let href = format!("data:image/png;base64,{}", base64(&keyed_truecolour_png(&[0x00, 0x00, 0x00, 0xFF, 0x00, 0x00])));
    let bytes = render_scene(scene_with_images(&[(&href, None)]));
    let text = lossy(&bytes);

    assert_eq!(text.matches("/Type /XObject /Subtype /Image").count(), 2, "the colour image plus its key mask: {text}");
    let mask = mask_number(&text);
    let alpha = mask_stream(&bytes, mask);
    assert_eq!(alpha, [255, 0, 255, 255], "only the keyed green pixel turns transparent");
    assert_eq!(alpha.iter().filter(|&&sample| sample == 0).count(), 1, "the mask carries exactly one zero byte");

    let color: [u8; 12] = [255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255];
    assert!(bytes.windows(color.len()).any(|window| window == color), "the colour plane keeps every sample: {text}");
    assert_xref_intact(&bytes);
}

#[test]
fn colour_key_that_keys_no_pixel_leaves_the_image_opaque() {
    let href = format!("data:image/png;base64,{}", base64(&keyed_truecolour_png(&[0x00, 0x01, 0x00, 0x02, 0x00, 0x03])));
    let bytes = render_scene(scene_with_images(&[(&href, None)]));
    let text = lossy(&bytes);

    assert_eq!(text.matches("/Type /XObject /Subtype /Image").count(), 1, "a key that misses invents no mask: {text}");
    assert!(!text.contains("/SMask"), "an all-opaque mask is never written: {text}");
    assert!(text.contains("/Length 12 >>"), "the colour plane still paints: {text}");
    assert_xref_intact(&bytes);
}

#[test]
fn colour_key_compares_in_the_source_sample_depth() {
    // `0x1200` narrows to the same high byte as the `0x1234` sample, so a
    // truncated comparison would key the first pixel while the full-depth
    // one matches nothing.
    let key = [0x12, 0x00, 0x56, 0x78, 0x9A, 0xBC];
    let png = png_fixture(2, 2, 16, 2, 0, Some(&key), &SIXTEEN_BIT_RGB_SCANLINES);
    let href = format!("data:image/png;base64,{}", base64(&png));
    let bytes = render_scene(scene_with_images(&[(&href, None)]));
    let text = lossy(&bytes);

    assert!(text.contains("/Length 12 >>"), "the 16-bit image still decodes: {text}");
    assert!(!text.contains("/SMask"), "a key that misses every full-depth sample invents no mask: {text}");
    assert_xref_intact(&bytes);
}

#[test]
fn greyscale_colour_key_becomes_a_soft_mask() {
    let href = format!("data:image/png;base64,{}", base64(&keyed_grey_png(&[0x00, 0x14])));
    let bytes = render_scene(scene_with_images(&[(&href, None)]));
    let text = lossy(&bytes);

    assert_eq!(text.matches("/Type /XObject /Subtype /Image").count(), 2, "the colour image plus its key mask: {text}");
    assert_eq!(text.matches("/ColorSpace /DeviceGray").count(), 2, "both planes are single-channel: {text}");
    let mask = mask_number(&text);
    assert_eq!(mask_stream(&bytes, mask), [255, 0, 255, 0], "both samples equal to the key turn transparent");

    let color: [u8; 4] = [10, 20, 30, 20];
    assert!(bytes.windows(color.len()).any(|window| window == color), "the colour plane keeps the grey samples: {text}");
    assert_xref_intact(&bytes);
}

/// Adam7 pass rectangles, as `(x origin, y origin, x step, y step)`.
const ADAM7_PASSES: [(usize, usize, usize, usize); 7] =
    [(0, 0, 8, 8), (4, 0, 8, 8), (0, 4, 4, 8), (2, 0, 4, 4), (0, 2, 2, 4), (1, 0, 2, 2), (0, 1, 1, 2)];

/// Filtered scanlines of a row-major image, one filter byte per row.
fn row_major_scanlines(pixel: impl Fn(usize, usize) -> Vec<u8>, width: usize, height: usize) -> Vec<u8> {
    let mut out = Vec::new();
    for y in 0..height {
        out.push(0);
        for x in 0..width {
            out.extend_from_slice(&pixel(x, y));
        }
    }
    out
}

/// Filtered scanlines of the same image stored in Adam7 pass order, one filter
/// byte per pass row.
fn adam7_scanlines(pixel: impl Fn(usize, usize) -> Vec<u8>, width: usize, height: usize) -> Vec<u8> {
    let mut out = Vec::new();
    for (x_origin, y_origin, x_step, y_step) in ADAM7_PASSES {
        let pass_width = width.saturating_sub(x_origin).div_ceil(x_step);
        let pass_height = height.saturating_sub(y_origin).div_ceil(y_step);
        if pass_width == 0 || pass_height == 0 {
            continue;
        }
        for row in 0..pass_height {
            out.push(0);
            for column in 0..pass_width {
                out.extend_from_slice(&pixel(x_origin + column * x_step, y_origin + row * y_step));
            }
        }
    }
    out
}

#[test]
fn adam7_interlaced_png_matches_its_row_major_twin() {
    let pixel = |x: usize, y: usize| vec![x as u8 * 60 + 1, y as u8 * 60 + 2, (x * 10 + y) as u8];
    let plain = png_fixture(3, 3, 8, 2, 0, None, &row_major_scanlines(pixel, 3, 3));
    let interlaced = png_fixture(3, 3, 8, 2, 1, None, &adam7_scanlines(pixel, 3, 3));
    let plain_href = format!("data:image/png;base64,{}", base64(&plain));
    let interlaced_href = format!("data:image/png;base64,{}", base64(&interlaced));
    let plain_bytes = render_scene(scene_with_images(&[(&plain_href, None)]));
    let interlaced_bytes = render_scene(scene_with_images(&[(&interlaced_href, None)]));
    let text = lossy(&interlaced_bytes);

    assert!(text.contains("/Width 3") && text.contains("/Height 3"), "the frame survives interlacing: {text}");
    assert!(text.contains("/Length 27 >>"), "a 3×3 RGB image writes 27 narrow samples: {text}");
    assert!(!text.contains("/SMask"), "interlacing invents no transparency: {text}");
    assert_eq!(
        image_stream(&interlaced_bytes),
        image_stream(&plain_bytes),
        "de-interlacing restores the same rows as the non-interlaced twin"
    );
    assert_xref_intact(&interlaced_bytes);
}

#[test]
fn adam7_interlaced_sixteen_bit_png_matches_its_row_major_twin() {
    let pixel = |x: usize, y: usize| vec![0x10 + x as u8, 0xAA, 0x20 + y as u8, 0xBB, 0x30 + x as u8, 0xCC];
    let plain = png_fixture(3, 3, 16, 2, 0, None, &row_major_scanlines(pixel, 3, 3));
    let interlaced = png_fixture(3, 3, 16, 2, 1, None, &adam7_scanlines(pixel, 3, 3));
    let plain_href = format!("data:image/png;base64,{}", base64(&plain));
    let interlaced_href = format!("data:image/png;base64,{}", base64(&interlaced));
    let plain_bytes = render_scene(scene_with_images(&[(&plain_href, None)]));
    let interlaced_bytes = render_scene(scene_with_images(&[(&interlaced_href, None)]));
    let stream = image_stream(&interlaced_bytes);

    assert_eq!(stream.len(), 27, "nine pixels narrow to three samples each");
    assert_eq!(stream, image_stream(&plain_bytes), "de-interlacing restores the same rows as the non-interlaced twin");
    assert_eq!(
        &stream[..9],
        [0x10, 0x20, 0x30, 0x11, 0x20, 0x31, 0x12, 0x20, 0x32],
        "the first row keeps its authored high bytes"
    );
    assert_xref_intact(&interlaced_bytes);
}

/// One page-level image slot carrying the authored source.
fn page_image(source: Option<&str>) -> DisplayItem {
    DisplayItem::Image(DisplayImage {
        x: 72.0,
        y: 260.0,
        width: 120.0,
        height: 60.0,
        alt: "figure".to_string(),
        source: source.map(str::to_string),
    })
}

/// Render one page that carries the given items.
fn render_items(items: Vec<DisplayItem>) -> Vec<u8> {
    render_pdf(&DisplayPresentation { pages: vec![page(595.0, 842.0, items)] }).expect("pdf render")
}

/// Inline `data:image/svg+xml` source holding one SVG document.
fn svg_data_uri(document: &str) -> String {
    format!("data:image/svg+xml;base64,{}", base64(document.as_bytes()))
}

/// Render one page-level slot carrying the given SVG document.
fn render_svg(document: &str) -> Vec<u8> {
    render_items(vec![page_image(Some(&svg_data_uri(document)))])
}

#[test]
fn embeds_a_page_image_as_a_raster_xobject() {
    let href = format!("data:image/png;base64,{}", base64(&tiny_png()));
    let bytes = render_items(vec![page_image(Some(&href))]);
    let text = lossy(&bytes);

    assert_eq!(text.matches("/Type /XObject /Subtype /Image").count(), 1, "one image object: {text}");
    assert!(text.contains("/Width 2") && text.contains("/Height 2"), "PNG frame reaches the dictionary: {text}");
    assert!(text.contains("/XObject << /Im1 "), "page resources register the image: {text}");
    assert!(text.contains("\n/Im1 Do\n"), "the content stream paints the image: {text}");
    assert!(!text.contains("[3 2] 0 d"), "a resolved page image replaces the dashed slot: {text}");
    assert!(text.contains("120 0 0 60 72 522 cm"), "positive y scale puts sample row zero on the box top edge: {text}");
    assert_xref_intact(&bytes);
}

#[test]
fn grey_alpha_png_keeps_both_planes() {
    let href = format!("data:image/png;base64,{}", base64(&grey_alpha_png()));
    let bytes = render_scene(scene_with_images(&[(&href, None)]));
    let text = lossy(&bytes);

    assert_eq!(text.matches("/Type /XObject /Subtype /Image").count(), 2, "one colour image and one mask: {text}");
    assert_eq!(text.matches("/ColorSpace /DeviceGray").count(), 2, "both planes are single-channel: {text}");
    let mask = mask_number(&text);
    assert!(text.contains(&format!("/SMask {mask} 0 R")), "the colour image references the mask: {text}");

    let color: [u8; 4] = [10, 20, 10, 20];
    assert!(bytes.windows(color.len()).any(|window| window == color), "the colour plane keeps the grey samples");
    assert_eq!(mask_stream(&bytes, mask), [0, 255, 0, 255], "the mask keeps the authored alpha samples");
    assert_xref_intact(&bytes);
}

#[test]
fn page_image_without_a_source_keeps_the_dashed_placeholder() {
    let bytes = render_items(vec![page_image(None)]);
    let text = lossy(&bytes);

    assert!(text.contains("[3 2] 0 d"), "a slot without a source keeps the dashed box: {text}");
    assert!(!text.contains("/Subtype /Image"), "no object is invented for an absent source: {text}");
    assert_xref_intact(&bytes);
}

#[test]
fn relative_image_resolves_against_the_base_directory() {
    let dir = ScratchDir::new("base");
    dir.write("figure.png", &tiny_png());
    let resources = PdfResources::new().with_base_dir(dir.path.clone());
    let bytes = render_scene_with(scene_with_images(&[("figure.png", None)]), &resources);
    let text = lossy(&bytes);

    assert!(text.contains("/Type /XObject /Subtype /Image"), "the base directory makes the sibling readable: {text}");
    assert!(text.contains("\n/Im1 Do\n"), "the content stream paints the picture: {text}");
    assert!(!text.contains("[3 2] 0 d"), "a resolved relative source replaces the placeholder");
    assert_xref_intact(&bytes);
}

#[test]
fn relative_image_without_a_base_directory_keeps_the_placeholder() {
    let absent = format!("vutex-pdf-cwd-absent-{}.png", std::process::id());
    let bytes = render_scene(scene_with_images(&[(&absent, None)]));
    let text = lossy(&bytes);

    assert!(text.contains("[3 2] 0 d"), "an unresolvable relative source keeps the dashed box: {text}");
    assert!(!text.contains("/Subtype /Image"), "no object is invented for an absent file: {text}");
    assert_xref_intact(&bytes);
}

#[test]
fn supplied_payload_embeds_a_remote_source() {
    let source = "https://example.com/figure.png";
    let bytes = render_scene(scene_with_images(&[(source, None)]));
    let refused = lossy(&bytes);
    assert!(!refused.contains("/Subtype /Image"), "a remote scheme resolves to nothing without a payload: {refused}");

    let resources = PdfResources::new().with_supplied(source, tiny_png());
    let bytes = render_scene_with(scene_with_images(&[(source, None)]), &resources);
    let text = lossy(&bytes);

    assert!(text.contains("/Type /XObject /Subtype /Image"), "the supplied bytes embed despite the scheme: {text}");
    assert!(text.contains("\n/Im1 Do\n"), "the content stream paints the picture: {text}");
    assert!(!text.contains("[3 2] 0 d"), "a supplied source replaces the placeholder");
    assert_xref_intact(&bytes);
}

#[test]
fn supplied_payload_that_is_not_an_image_keeps_the_placeholder() {
    let source = "https://example.com/broken.png";
    let resources = PdfResources::new().with_supplied(source, b"not an image".to_vec());
    let bytes = render_scene_with(scene_with_images(&[(source, None)]), &resources);
    let text = lossy(&bytes);

    assert!(text.contains("[3 2] 0 d"), "an undecodable payload keeps the dashed box: {text}");
    assert!(!text.contains("/Subtype /Image"), "no raster is invented from a non-image payload: {text}");
    assert_xref_intact(&bytes);
}

#[test]
fn default_resource_context_matches_the_previous_render() {
    let href = format!("data:image/png;base64,{}", base64(&tiny_png()));
    let presentation = DisplayPresentation { pages: vec![page(595.0, 842.0, vec![scene_with_images(&[(&href, None)])])] };
    let previous = render_pdf(&presentation).expect("pdf render");
    let explicit =
        render_pdf_with_resources(&presentation, &PdfFontPolicy::Discover, &PdfResources::default()).expect("pdf render");

    assert_eq!(previous, explicit, "an empty context reproduces the historical resolution order");
    assert!(lossy(&previous).contains("/Type /XObject /Subtype /Image"), "inline sources still embed");
}

#[test]
fn repeated_transparent_references_share_one_mask() {
    let href = format!("data:image/png;base64,{}", base64(&alpha_png()));
    let bytes = render_scene(scene_with_images(&[(&href, None), (&href, None)]));
    let text = lossy(&bytes);

    assert_eq!(text.matches("/Type /XObject /Subtype /Image").count(), 2, "one colour image plus one mask: {text}");
    assert_eq!(text.matches("/SMask ").count(), 1, "/SMask is written once: {text}");
    assert_eq!(text.matches("\n/Im1 Do\n").count(), 2, "both placements paint the colour image");
    assert!(!text.contains("/Im2"), "the mask never becomes its own resource name");
    assert_eq!(text.matches("/XObject <<").count(), 1, "page resources list the colour image once");
    assert_xref_intact(&bytes);
}

fn repeated_page_image_sources_share_one_xobject() {
    let href = format!("data:image/png;base64,{}", base64(&tiny_png()));
    let bytes = render_items(vec![page_image(Some(&href)), page_image(Some(&href))]);
    let text = lossy(&bytes);

    assert_eq!(text.matches("/Type /XObject /Subtype /Image").count(), 1, "one object per source: {text}");
    assert_eq!(text.matches("\n/Im1 Do\n").count(), 2, "both slots select the same object");
    assert!(!text.contains("/Im2"), "the repeated source gets no second name");
    assert_xref_intact(&bytes);
}

#[test]
fn svg_icon_paints_vector_geometry_without_an_image_xobject() {
    let document = r##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24">
  <title>icon</title>
  <rect x="2" y="3" width="10" height="8" fill="#ff0000" stroke="#000000"/>
  <circle cx="18" cy="18" r="3" fill="currentColor"/>
</svg>"##;
    let bytes = render_svg(document);
    let text = lossy(&bytes);

    assert!(text.contains("\nB\n"), "the rect paints fill and stroke: {text}");
    assert!(text.contains("\nf\n"), "the circle paints fill: {text}");
    assert!(text.contains("1 0 0 rg"), "the hex fill reaches the operator: {text}");
    assert!(!text.contains("/XObject"), "vector artwork registers no XObject: {text}");
    assert!(!text.contains("/Subtype /Image"), "no raster is invented for an SVG: {text}");
    assert!(!text.contains("[3 2] 0 d"), "a supported SVG replaces the placeholder: {text}");
    assert!(balanced_graphics_states(&text), "every q is closed: {text}");
    assert_xref_intact(&bytes);
}

#[test]
fn svg_path_lifts_lines_and_cubics_and_closes() {
    let document = r##"<svg width="24" height="24"><path d="M0 0 L10 0 C10 5 5 10 0 10 Z" fill="none" stroke="#000" stroke-width="2"/></svg>"##;
    let text = lossy(&render_svg(document));

    assert!(text.contains("10 0 l\n"), "L lifts to a line: {text}");
    assert!(text.contains(" c\n"), "C lifts to a cubic Bézier: {text}");
    assert!(text.contains("h\n"), "Z closes the path: {text}");
    assert!(text.contains("2 w\n"), "stroke-width reaches the operator: {text}");
    assert!(text.contains("\nS\n"), "a stroke-only path uses S: {text}");
    assert!(!text.contains("[3 2] 0 d"), "a supported path replaces the placeholder: {text}");
}

#[test]
fn svg_group_transform_composes_left_to_right() {
    let document = r##"<svg width="24" height="24"><g transform="translate(2 4) scale(3)"><rect width="2" height="2" fill="#000"/></g></svg>"##;
    let text = lossy(&render_svg(document));

    assert!(text.contains("3 0 0 3 2 4 cm\n"), "translate then scale composes to translate × scale: {text}");
}

#[test]
fn svg_stroke_and_fill_toggles_choose_their_operators() {
    let stroked = lossy(&render_svg(
        r##"<svg width="24" height="24"><rect width="10" height="10" fill="none" stroke="#000" stroke-width="3"/></svg>"##,
    ));
    assert!(stroked.contains("3 w\n"), "stroke width reaches the operator: {stroked}");
    assert!(stroked.contains("\nS\n"), "fill=none paints with S: {stroked}");
    assert!(!stroked.contains("\nf\n"), "fill=none never paints a fill: {stroked}");

    let filled =
        lossy(&render_svg(r##"<svg width="24" height="24"><rect width="10" height="10" fill="#fff" stroke="none"/></svg>"##));
    assert!(filled.contains("1 1 1 rg"), "stroke=none still paints the white fill: {filled}");
    assert!(filled.contains("\nf\n"), "stroke=none paints with f: {filled}");
    assert!(!filled.contains("\nS\n"), "stroke=none never paints a stroke: {filled}");
}

#[test]
fn svg_view_box_and_slot_keep_the_aspect_ratio() {
    // A 4:1 viewBox fitted into a square element letterboxes vertically, and
    // the square scene then centres inside the 2:1 page slot.
    let document = r##"<svg width="48" height="48" viewBox="0 0 96 24"><rect width="96" height="24" fill="#000"/></svg>"##;
    let text = lossy(&render_svg(document));

    assert!(text.contains("0.5 0 0 0.5 0 18 cm\n"), "the viewBox scales uniformly and centres: {text}");
    assert!(text.contains("1.25 0 0 -1.25 102 582 cm\n"), "the slot fits uniformly, centres, and flips page y: {text}");
}

#[test]
fn broken_svg_falls_back_to_the_dashed_placeholder() {
    let truncated_path = render_svg(r##"<svg width="24" height="24"><path d="M0 0 L"/></svg>"##);
    let text = lossy(&truncated_path);
    assert!(text.contains("[3 2] 0 d"), "an incomplete path keeps the dashed box: {text}");
    assert!(!text.contains("/Subtype /Image"), "a broken SVG invents no raster: {text}");
    assert_xref_intact(&truncated_path);

    let unterminated = render_svg(r##"<svg width="24" height="24"><rect width="4" height="4""##);
    let text = lossy(&unterminated);
    assert!(text.contains("[3 2] 0 d"), "malformed XML keeps the dashed box: {text}");
    assert_xref_intact(&unterminated);
}

#[test]
fn repeated_svg_sources_parse_once_and_paint_once_per_slot() {
    let href = svg_data_uri(r##"<svg width="24" height="24"><rect width="24" height="24" fill="#000"/></svg>"##);
    let bytes = render_items(vec![page_image(Some(&href)), page_image(Some(&href))]);
    let text = lossy(&bytes);

    // Both slots paint the one parsed scene, without a second registration.
    assert_eq!(text.matches("\nf\n").count(), 2, "each slot paints exactly once: {text}");
    assert!(!text.contains("/XObject"), "repeated SVG sources register nothing: {text}");
    assert!(!text.contains("[3 2] 0 d"), "neither slot falls back: {text}");
    assert_xref_intact(&bytes);
}

#[test]
fn svg_scene_image_paints_in_scene_coordinates() {
    let href = svg_data_uri(r##"<svg width="24" height="24"><rect width="24" height="24" fill="#000"/></svg>"##);
    let bytes = render_scene(scene_with_images(&[(&href, None)]));
    let text = lossy(&bytes);

    assert!(text.contains("\nf\n"), "the nested scene paints its geometry: {text}");
    assert!(!text.contains("/Subtype /Image"), "a scene SVG registers no raster: {text}");
    assert!(!text.contains("[3 2] 0 d"), "a supported scene SVG replaces the placeholder: {text}");
    assert_xref_intact(&bytes);
}

/// The image matrix reverses the sample rows under the scene's own y flip.
///
/// The scene root maps scene-space y downward with a negative `d`. The image
/// matrix must then also be negative and translate the unit square so its
/// bottom edge sits at `rect.y + rect.height`; the product of the two negative
/// scales is positive, so sample row zero lands on the page's upper edge.
fn assert_image_orientation(text: &str) {
    assert!(text.contains("2 0 0 -2 72 742 cm"), "scene root flips y once: {text}");
    assert!(text.contains("20 0 0 -10 10 20 cm"), "image matrix flips sample rows: {text}");
}
