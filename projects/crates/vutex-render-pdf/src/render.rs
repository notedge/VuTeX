//! PDF output: a minimal, dependency-free PDF 1.7 writer.
//!
//! The writer paints the same display primitives as the SVG backend with the
//! base-14 standard fonts, so no font program has to be embedded. Coordinates
//! arrive with the origin at the page top-left corner and are flipped to PDF's
//! bottom-left origin by subtracting them from the page height.

use std::fmt::Write as _;

use vutex_presentation::display::{
    DisplayImage, DisplayItem, DisplayPage, DisplayPresentation, DisplayRect, DisplayRule, DisplayTextRun,
};
use vutex_render::RenderBackend;
use vutex_types::{Diagnostic, Severity};

/// Base-14 fonts this backend can emit, in canonical resource order.
const BASE_FONTS: [&str; 3] = ["Courier", "Helvetica", "Times-Roman"];

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

/// Base-14 font for a display font family.
fn base_font_for(family: &str) -> &'static str {
    let lower = family.to_ascii_lowercase();
    if lower.contains("mono") {
        "Courier"
    }
    else if lower.contains("sans") {
        "Helvetica"
    }
    else {
        "Times-Roman"
    }
}

/// Distinct base-14 fonts referenced by any text run, in canonical order.
fn used_fonts(presentation: &DisplayPresentation) -> Vec<&'static str> {
    let mut used: Vec<&'static str> = Vec::new();
    for page in &presentation.pages {
        for item in &page.items {
            if let DisplayItem::TextRun(run) = item {
                let base = base_font_for(&run.font_family);
                if !used.contains(&base) {
                    used.push(base);
                }
            }
        }
    }
    BASE_FONTS.iter().copied().filter(|base| used.contains(base)).collect()
}

/// Object strings in object-number order; index `i` is object `i + 1`.
fn build_objects(presentation: &DisplayPresentation, fonts: &[&'static str]) -> Vec<String> {
    let page_count = presentation.pages.len();
    let first_font = 3 + 2 * page_count;
    let font_objects: Vec<(&'static str, usize)> =
        fonts.iter().enumerate().map(|(index, base)| (*base, first_font + index)).collect();
    let resources = font_resource_dict(&font_objects);

    let mut objects = vec![catalog_object(), pages_object(page_count)];
    for (index, page) in presentation.pages.iter().enumerate() {
        let content_number = 4 + 2 * index;
        let content = page_content(page, &font_objects);
        objects.push(page_object(page, &resources, content_number));
        objects.push(content_object(&content));
    }
    for (base, _) in &font_objects {
        objects.push(font_object(base));
    }
    objects
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

fn page_object(page: &DisplayPage, resources: &str, content_number: usize) -> String {
    format!(
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Resources {resources} /Contents {content_number} 0 R >>",
        num(page.width),
        num(page.height),
    )
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
    }
}

fn write_text(out: &mut String, page_height: f64, run: &DisplayTextRun, fonts: &[(&'static str, usize)]) {
    if !all_finite(&[run.x, run.y, run.size]) {
        return;
    }
    let base = base_font_for(&run.font_family);
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

fn all_finite(values: &[f64]) -> bool {
    values.iter().copied().all(f64::is_finite)
}

/// Escape a string for a PDF literal string and encode it for WinAnsi.
///
/// `\`, `(`, and `)` are backslash-escaped, printable ASCII passes through,
/// other Latin-1 code points become three-digit octal escapes, and code points
/// outside Latin-1 become `?` because WinAnsiEncoding cannot represent them.
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
            _ => out.push('?'),
        }
    }
    out
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
