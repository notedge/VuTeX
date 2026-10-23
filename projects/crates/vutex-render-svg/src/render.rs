//! SVG output: one stacked page group per display page.

use vutex_presentation::display::{DisplayItem, DisplayPresentation};
use vutex_render::RenderBackend;
use vutex_types::{Diagnostic, math::FontStyle};

/// Render every display page into one SVG document.
///
/// Pages are stacked vertically in document order; each page is a
/// `<g data-page="n">` group translated to its slot. Item coordinates stay in
/// points with the origin at the page's top-left corner.
pub fn render_svg(presentation: &DisplayPresentation) -> Result<String, Diagnostic> {
    let backend = SvgBackend;
    backend.render_display(presentation).map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

struct SvgBackend;

impl RenderBackend for SvgBackend {
    fn render_display(&self, presentation: &DisplayPresentation) -> Result<Vec<u8>, Diagnostic> {
        Ok(render_document(presentation).into_bytes())
    }
}

fn render_document(presentation: &DisplayPresentation) -> String {
    let width = presentation.pages.iter().map(|page| page.width).fold(0.0, f64::max);
    let height: f64 = presentation.pages.iter().map(|page| page.height).sum();
    let mut out = String::new();
    out.push_str(&format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}" role="img">"##,
        width = num(width),
        height = num(height),
    ));
    out.push_str("\n<title>VuTeX document</title>\n");
    let mut offset = 0.0;
    for (index, page) in presentation.pages.iter().enumerate() {
        out.push_str(&format!(r##"<g data-page="{}" transform="translate(0, {})">"##, index + 1, num(offset),));
        out.push('\n');
        out.push_str(&format!(
            r##"<rect x="0" y="0" width="{}" height="{}" fill="#ffffff"/>"##,
            num(page.width),
            num(page.height),
        ));
        out.push('\n');
        for item in &page.items {
            write_item(&mut out, item);
        }
        out.push_str("</g>\n");
        offset += page.height;
    }
    out.push_str("</svg>\n");
    out
}

fn write_item(out: &mut String, item: &DisplayItem) {
    match item {
        DisplayItem::TextRun(run) => {
            out.push_str(&format!(
                r##"<text x="{}" y="{}" font-size="{}" font-family="{}"{} xml:space="preserve">{}</text>"##,
                num(run.x),
                num(run.y),
                num(run.size),
                escape_attribute(&run.font_family),
                style_attributes(run.style),
                escape_text(&run.text),
            ));
        }
        DisplayItem::Rule(rule) => {
            out.push_str(&format!(
                r##"<line x1="{}" y1="{}" x2="{}" y2="{}" stroke="#111111" stroke-width="{}"/>"##,
                num(rule.x1),
                num(rule.y1),
                num(rule.x2),
                num(rule.y2),
                num(rule.thickness),
            ));
        }
        DisplayItem::Rect(rect) if rect.filled => {
            out.push_str(&format!(
                r##"<rect x="{}" y="{}" width="{}" height="{}" fill="#f2f2f2"/>"##,
                num(rect.x),
                num(rect.y),
                num(rect.width),
                num(rect.height),
            ));
        }
        DisplayItem::Rect(rect) => {
            out.push_str(&format!(
                r##"<rect x="{}" y="{}" width="{}" height="{}" fill="none" stroke="#999999" stroke-width="0.5"/>"##,
                num(rect.x),
                num(rect.y),
                num(rect.width),
                num(rect.height),
            ));
        }
        DisplayItem::Image(image) => {
            out.push_str(&format!(
                r##"<rect x="{}" y="{}" width="{}" height="{}" fill="#fafafa" stroke="#999999" stroke-width="0.5" stroke-dasharray="3 2" data-alt="{}"/>"##,
                num(image.x),
                num(image.y),
                num(image.width),
                num(image.height),
                escape_attribute(&image.alt),
            ));
        }
    }
    out.push('\n');
}

fn num(value: f64) -> String {
    format!("{value:.3}")
}

/// SVG presentation attributes for a text face style.
///
/// Unstyled runs append nothing, so plain body text keeps the exact attribute
/// set it had before styles existed.
fn style_attributes(style: FontStyle) -> &'static str {
    match style {
        FontStyle::Bold => r#" font-weight="700""#,
        FontStyle::Italic => r#" font-style="italic""#,
        _ => "",
    }
}

fn escape_text(value: &str) -> String {
    value.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn escape_attribute(value: &str) -> String {
    escape_text(value).replace('"', "&quot;").replace('\'', "&#39;")
}
