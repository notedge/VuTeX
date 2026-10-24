//! PDF output: a minimal, dependency-free PDF 1.7 writer.
//!
//! The writer paints the same display primitives as the SVG backend with the
//! base-14 standard fonts, so no font program has to be embedded. Coordinates
//! arrive with the origin at the page top-left corner and are flipped to PDF's
//! bottom-left origin by subtracting them from the page height.

use std::fmt::Write as _;

use vutex_presentation::display::{
    DisplayGraphic, DisplayImage, DisplayItem, DisplayPage, DisplayPresentation, DisplayRect, DisplayRule, DisplayTextRun,
};
use vutex_render::{RenderBackend, is_safe_href};
use vutex_types::{Diagnostic, Severity, math::FontStyle};

/// Base-14 fonts this backend can emit, in canonical resource order.
///
/// The table covers the three standard families in their regular, bold,
/// italic, and bold-italic faces, so `/F1../Fn` stays stable no matter which
/// styles a document uses.
const BASE_FONTS: [&str; 12] = [
    "Courier",
    "Courier-Bold",
    "Courier-BoldOblique",
    "Courier-Oblique",
    "Helvetica",
    "Helvetica-Bold",
    "Helvetica-BoldOblique",
    "Helvetica-Oblique",
    "Times-Roman",
    "Times-Bold",
    "Times-BoldItalic",
    "Times-Italic",
];

/// Run extent above the baseline, as a fraction of the font size.
///
/// This mirrors the layout contract that places every baseline at
/// `line top + 0.8 * size`, so a link rectangle covers the whole line box.
const ASCENT: f64 = 0.8;
/// Run extent below the baseline, as a fraction of the font size.
///
/// The base-14 faces have no embedded metrics here, so the descent is the
/// stacked complement of [`ASCENT`] rather than a per-font value.
const DESCENDER: f64 = 0.2;

/// PDF renderer. Consumes only the display presentation IR.
pub fn render_pdf(presentation: &DisplayPresentation) -> Result<Vec<u8>, Diagnostic> {
    let backend = PdfBackend;
    backend.render_display(presentation)
}

struct PdfBackend;

impl RenderBackend for PdfBackend {
    fn render_display(&self, presentation: &DisplayPresentation) -> Result<Vec<u8>, Diagnostic> {
        render_document(presentation)
    }
}

fn render_document(presentation: &DisplayPresentation) -> Result<Vec<u8>, Diagnostic> {
    if presentation.pages.is_empty() {
        return Err(Diagnostic {
            code: "vutex.render.pdf.no_pages".into(),
            severity: Severity::Error,
            message: "PDF output needs at least one display page with page geometry".into(),
        });
    }
    let fonts = used_fonts(presentation);
    let objects = build_objects(presentation, &fonts);
    Ok(assemble(&objects))
}

/// Base-14 font for a display font family and face style.
///
/// Families resolve by substring, so CSS stacks such as `sans-serif` keep
/// working; a style the base-14 set cannot express (math and plain sans/mono
/// labels) falls back to the family's regular face.
fn base_font_for(family: &str, style: FontStyle) -> &'static str {
    let lower = family.to_ascii_lowercase();
    if lower.contains("mono") {
        match style {
            FontStyle::Bold => "Courier-Bold",
            FontStyle::Italic => "Courier-Oblique",
            _ => "Courier",
        }
    }
    else if lower.contains("sans") {
        match style {
            FontStyle::Bold => "Helvetica-Bold",
            FontStyle::Italic => "Helvetica-Oblique",
            _ => "Helvetica",
        }
    }
    else {
        match style {
            FontStyle::Bold => "Times-Bold",
            FontStyle::Italic => "Times-Italic",
            _ => "Times-Roman",
        }
    }
}

/// Distinct base-14 fonts referenced by any text run, in canonical order.
fn used_fonts(presentation: &DisplayPresentation) -> Vec<&'static str> {
    let mut used: Vec<&'static str> = Vec::new();
    for page in &presentation.pages {
        for item in &page.items {
            if let DisplayItem::TextRun(run) = item {
                let base = base_font_for(&run.font_family, run.style);
                if !used.contains(&base) {
                    used.push(base);
                }
            }
        }
    }
    BASE_FONTS.iter().copied().filter(|base| used.contains(base)).collect()
}

/// A link annotation produced by one safe text-run destination.
#[derive(Debug, Clone, PartialEq)]
struct LinkAnnotation {
    /// Lower-left x, in PDF user space.
    x1: f64,
    /// Lower-left y, in PDF user space.
    y1: f64,
    /// Upper-right x, in PDF user space.
    x2: f64,
    /// Upper-right y, in PDF user space.
    y2: f64,
    /// Destination written into the `/URI` action.
    uri: String,
    /// Hover text written into `/Contents`, when the run carried a title.
    title: Option<String>,
}

/// Object strings in object-number order; index `i` is object `i + 1`.
fn build_objects(presentation: &DisplayPresentation, fonts: &[&'static str]) -> Vec<String> {
    let page_count = presentation.pages.len();
    let first_font = 3 + 2 * page_count;
    let first_annotation = first_font + fonts.len();
    let font_objects: Vec<(&'static str, usize)> =
        fonts.iter().enumerate().map(|(index, base)| (*base, first_font + index)).collect();
    let resources = font_resource_dict(&font_objects);
    let page_links: Vec<Vec<LinkAnnotation>> = presentation.pages.iter().map(page_links).collect();
    let mut next_annotation = first_annotation;
    let mut annotation_numbers: Vec<Vec<usize>> = Vec::with_capacity(page_count);
    for links in &page_links {
        annotation_numbers.push((0..links.len()).map(|index| next_annotation + index).collect());
        next_annotation += links.len();
    }

    let mut objects = vec![catalog_object(), pages_object(page_count)];
    for (index, page) in presentation.pages.iter().enumerate() {
        let content_number = 4 + 2 * index;
        let content = page_content(page, &font_objects);
        objects.push(page_object(page, &resources, content_number, &annotation_numbers[index]));
        objects.push(content_object(&content));
    }
    for (base, _) in &font_objects {
        objects.push(font_object(base));
    }
    for links in &page_links {
        for link in links {
            objects.push(annotation_object(link));
        }
    }
    objects
}

/// Link annotations for every safe destination on one page, in paint order.
///
/// The rectangle is the run's glyph box: the baseline sits at `page_height -
/// y`, the top at `0.8 * size` above it, and the bottom at `0.2 * size` below
/// it. Boxes are clipped to the page MediaBox so a run that overflows the page
/// never produces an annotation outside it.
fn page_links(page: &DisplayPage) -> Vec<LinkAnnotation> {
    let mut links = Vec::new();
    for item in &page.items {
        let DisplayItem::TextRun(run) = item
        else {
            continue;
        };
        let Some(uri) = run.href.as_deref().filter(|href| is_safe_href(href))
        else {
            continue;
        };
        if !all_finite(&[run.x, run.y, run.size, run.width]) {
            continue;
        }
        let x1 = run.x.clamp(0.0, page.width);
        let x2 = (run.x + run.width).clamp(0.0, page.width);
        let y1 = (page.height - (run.y + DESCENDER * run.size)).clamp(0.0, page.height);
        let y2 = (page.height - (run.y - ASCENT * run.size)).clamp(0.0, page.height);
        if x2 <= x1 || y2 <= y1 {
            continue;
        }
        links.push(LinkAnnotation { x1, y1, x2, y2, uri: uri.to_string(), title: run.title.clone() });
    }
    links
}

fn catalog_object() -> String {
    "<< /Type /Catalog /Pages 2 0 R >>".to_string()
}

fn pages_object(page_count: usize) -> String {
    let kids: Vec<String> = (0..page_count).map(|index| format!("{} 0 R", 3 + 2 * index)).collect();
    format!("<< /Type /Pages /Kids [{}] /Count {page_count} >>", kids.join(" "))
}

fn font_resource_dict(fonts: &[(&'static str, usize)]) -> String {
    let entries: Vec<String> =
        fonts.iter().enumerate().map(|(index, (_, object))| format!("/F{} {object} 0 R", index + 1)).collect();
    format!("<< /Font << {} >> >>", entries.join(" "))
}

fn page_object(page: &DisplayPage, resources: &str, content_number: usize, annotations: &[usize]) -> String {
    let mut object = format!(
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Resources {resources} /Contents {content_number} 0 R",
        num(page.width),
        num(page.height),
    );
    if !annotations.is_empty() {
        let references: Vec<String> = annotations.iter().map(|number| format!("{number} 0 R")).collect();
        let _ = write!(object, " /Annots [{}]", references.join(" "));
    }
    object.push_str(" >>");
    object
}

/// Link annotation: a URI action over the run's glyph rectangle, carrying the
/// authored hover text as `/Contents` when the run had one.
fn annotation_object(link: &LinkAnnotation) -> String {
    let mut object = format!(
        "<< /Type /Annot /Subtype /Link /Rect [{} {} {} {}] /Border [0 0 0] /A << /S /URI /URI ({}) >>",
        num(link.x1),
        num(link.y1),
        num(link.x2),
        num(link.y2),
        // Same literal-string escaping as text, so a destination containing
        // parentheses or backslashes cannot break the dictionary syntax.
        escape_text(&link.uri),
    );
    if let Some(title) = &link.title {
        let _ = write!(object, " /Contents ({})", escape_text(title));
    }
    object.push_str(" >>");
    object
}

fn font_object(base: &str) -> String {
    format!("<< /Type /Font /Subtype /Type1 /BaseFont /{base} /Encoding /WinAnsiEncoding >>")
}

fn content_object(content: &str) -> String {
    format!("<< /Length {} >>\nstream\n{content}endstream", content.len())
}

/// Content stream operators for one page; always ends with a newline.
fn page_content(page: &DisplayPage, fonts: &[(&'static str, usize)]) -> String {
    let mut out = String::new();
    for item in &page.items {
        write_item(&mut out, page.height, item, fonts);
    }
    if out.is_empty() {
        out.push('\n');
    }
    out
}

fn write_item(out: &mut String, page_height: f64, item: &DisplayItem, fonts: &[(&'static str, usize)]) {
    match item {
        DisplayItem::TextRun(run) => write_text(out, page_height, run, fonts),
        DisplayItem::Rule(rule) => write_rule(out, page_height, rule),
        DisplayItem::Rect(rect) => write_rect(out, page_height, rect),
        DisplayItem::Image(image) => write_image(out, page_height, image),
        DisplayItem::Graphic(graphic) => write_graphic(out, page_height, graphic),
    }
}

fn write_text(out: &mut String, page_height: f64, run: &DisplayTextRun, fonts: &[(&'static str, usize)]) {
    if !all_finite(&[run.x, run.y, run.size]) {
        return;
    }
    let base = base_font_for(&run.font_family, run.style);
    let position = fonts.iter().position(|(font, _)| *font == base).unwrap_or(0);
    let y = page_height - run.y;
    let _ = write!(
        out,
        "BT\n/F{} {} Tf\n1 0 0 1 {} {} Tm\n({}) Tj\nET\n",
        position + 1,
        num(run.size),
        num(run.x),
        num(y),
        escape_text(&run.text),
    );
}

fn write_rule(out: &mut String, page_height: f64, rule: &DisplayRule) {
    if !all_finite(&[rule.x1, rule.y1, rule.x2, rule.y2, rule.thickness]) {
        return;
    }
    let _ = write!(
        out,
        "0.067 G\n{} w\n{} {} m\n{} {} l\nS\n",
        num(rule.thickness),
        num(rule.x1),
        num(page_height - rule.y1),
        num(rule.x2),
        num(page_height - rule.y2),
    );
}

fn write_rect(out: &mut String, page_height: f64, rect: &DisplayRect) {
    if !all_finite(&[rect.x, rect.y, rect.width, rect.height]) {
        return;
    }
    let y = page_height - (rect.y + rect.height);
    if rect.filled {
        let _ = write!(out, "0.949 g\n{} {} {} {} re\nf\n", num(rect.x), num(y), num(rect.width), num(rect.height));
    }
    else {
        let _ = write!(out, "0.6 G\n0.5 w\n{} {} {} {} re\nS\n", num(rect.x), num(y), num(rect.width), num(rect.height));
    }
}

fn write_image(out: &mut String, page_height: f64, image: &DisplayImage) {
    if !all_finite(&[image.x, image.y, image.width, image.height]) {
        return;
    }
    let y = page_height - (image.y + image.height);
    let _ = write!(
        out,
        "0.6 G\n0.5 w\n[3 2] 0 d\n{} {} {} {} re\nS\n[] 0 d\n",
        num(image.x),
        num(y),
        num(image.width),
        num(image.height),
    );
}

fn write_graphic(out: &mut String, page_height: f64, graphic: &DisplayGraphic) {
    // The PDF backend has no vector scene painter yet, so it reserves the same
    // dashed box as an image slot instead of emitting guessed geometry.
    if !all_finite(&[graphic.x, graphic.y, graphic.width, graphic.height]) {
        return;
    }
    let y = page_height - (graphic.y + graphic.height);
    let _ = write!(
        out,
        "0.6 G\n0.5 w\n[3 2] 0 d\n{} {} {} {} re\nS\n[] 0 d\n",
        num(graphic.x),
        num(y),
        num(graphic.width),
        num(graphic.height),
    );
}

fn all_finite(values: &[f64]) -> bool {
    values.iter().copied().all(f64::is_finite)
}

/// Escape a string for a PDF literal string and encode it for WinAnsi.
///
/// `\`, `(`, and `)` are backslash-escaped, printable ASCII passes through,
/// Latin-1 code points become three-digit octal escapes, code points that
/// WinAnsi reaches through its upper range (typographic punctuation, the
/// bullet, the euro sign) are remapped to their WinAnsi byte, and everything
/// else becomes `?` because the encoding cannot represent it.
fn escape_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '(' => out.push_str("\\("),
            ')' => out.push_str("\\)"),
            ' '..='~' => out.push(ch),
            ch if (ch as u32) <= 0xFF => {
                let _ = write!(out, "\\{:03o}", ch as u32);
            }
            ch => match winansi_byte(ch) {
                Some(byte) => {
                    let _ = write!(out, "\\{byte:03o}");
                }
                None => out.push('?'),
            },
        }
    }
    out
}

/// WinAnsi byte for a code point above Latin-1 that the encoding still covers.
///
/// WinAnsiEncoding keeps the Latin-1 positions but replaces the `0x80..=0x9F`
/// block with printable punctuation, so code points such as U+2022 BULLET
/// (the list marker the layout pass emits) have a byte of their own instead of
/// degrading to `?`.
fn winansi_byte(ch: char) -> Option<u8> {
    let byte = match ch {
        '\u{20AC}' => 0x80,
        '\u{201A}' => 0x82,
        '\u{0192}' => 0x83,
        '\u{201E}' => 0x84,
        '\u{2026}' => 0x85,
        '\u{2020}' => 0x86,
        '\u{2021}' => 0x87,
        '\u{02C6}' => 0x88,
        '\u{2030}' => 0x89,
        '\u{0160}' => 0x8A,
        '\u{2039}' => 0x8B,
        '\u{0152}' => 0x8C,
        '\u{017D}' => 0x8E,
        '\u{2018}' => 0x91,
        '\u{2019}' => 0x92,
        '\u{201C}' => 0x93,
        '\u{201D}' => 0x94,
        '\u{2022}' => 0x95,
        '\u{2013}' => 0x96,
        '\u{2014}' => 0x97,
        '\u{02DC}' => 0x98,
        '\u{2122}' => 0x99,
        '\u{0161}' => 0x9A,
        '\u{203A}' => 0x9B,
        '\u{0153}' => 0x9C,
        '\u{017E}' => 0x9E,
        '\u{0178}' => 0x9F,
        _ => return None,
    };
    Some(byte)
}

/// Stable decimal rendering: at most three decimals, no trailing zeros, no `-0`.
fn num(value: f64) -> String {
    let text = format!("{value:.3}");
    let trimmed = text.trim_end_matches('0').trim_end_matches('.');
    if trimmed.is_empty() || trimmed == "-0" { "0".to_string() } else { trimmed.to_string() }
}

/// Serialize objects into a complete PDF file with a byte-exact xref table.
fn assemble(objects: &[String]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n");
    let mut offsets = Vec::with_capacity(objects.len());
    for (index, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        out.extend_from_slice(body.as_bytes());
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref_offset = out.len();
    let size = objects.len() + 1;
    out.extend_from_slice(format!("xref\n0 {size}\n").as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for offset in &offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n").as_bytes());
    out
}
