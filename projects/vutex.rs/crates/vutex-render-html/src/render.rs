//! HTML output for fixed-geometry snapshots and semantic reflow.

use vutex_presentation::{
    display::{DisplayItem, DisplayPath, DisplayPresentation, DisplayTextRun},
    flow::{FlowBlock, FlowPresentation, FlowRun, FlowTableRow},
};
use vutex_render::is_safe_href;
use vutex_types::{Diagnostic, Severity, math::FontStyle};

/// Snapshot HTML maps fixed layout geometry into absolute-positioned markup.
///
/// Every page becomes a `data-vutex-page` section with point-sized geometry;
/// every primitive becomes a `data-vutex-item` element carrying the same text.
pub fn render_html_snapshot(presentation: &DisplayPresentation) -> Result<String, Diagnostic> {
    let mut out = String::new();
    out.push_str(
        "<!doctype html>\n<html lang=\"und\">\n<head>\n<meta charset=\"utf-8\">\n<title>VuTeX snapshot</title>\n<style>\n\
body{margin:0;padding:24px;background:#f5f5f5}\
.vutex-page{position:relative;margin:0 auto 24px;background:#fff;box-shadow:0 0 4px rgba(0,0,0,.15)}\
.vutex-text{position:absolute;white-space:pre;line-height:1}\
.vutex-rule{position:absolute;background:#111}\
.vutex-image{position:absolute;box-sizing:border-box;border:1px dashed #999;color:#666;font:8pt sans-serif}\
.vutex-graphic{position:absolute;box-sizing:border-box;border:1px dashed #999;color:#666;font:8pt sans-serif}\
</style>\n</head>\n<body>\n",
    );
    for (index, page) in presentation.pages.iter().enumerate() {
        out.push_str(&format!(
            "<section class=\"vutex-page\" data-vutex-page=\"{}\" style=\"width:{}pt;height:{}pt\">\n",
            index + 1,
            num(page.width),
            num(page.height),
        ));
        for item in &page.items {
            write_item(&mut out, item);
        }
        out.push_str("</section>\n");
    }
    out.push_str("</body>\n</html>\n");
    Ok(out)
}

/// Reflow HTML maps semantic flow presentation into responsive markup.
///
/// Blocks carry `data-vutex-block` roles so hosts can restyle them, and all
/// text stays in the document flow for accessibility and text extraction.
pub fn render_html_reflow(presentation: &FlowPresentation) -> Result<String, Diagnostic> {
    if !is_valid_tag(&presentation.root_tag) {
        return Err(Diagnostic {
            code: "vutex.render.html.reflow.invalid-root-tag".into(),
            severity: Severity::Error,
            message: format!("flow root tag `{}` is not a plain element name", presentation.root_tag),
        });
    }
    let mut out = String::new();
    out.push_str(
        "<!doctype html>\n<html lang=\"und\">\n<head>\n<meta charset=\"utf-8\">\n<title>VuTeX document</title>\n</head>\n<body>\n",
    );
    out.push_str(&format!("<{} class=\"vutex-flow\" data-vutex=\"flow\">\n", presentation.root_tag,));
    for block in &presentation.blocks {
        write_flow_block(&mut out, block);
    }
    out.push_str(&format!("</{}>\n</body>\n</html>\n", presentation.root_tag));
    Ok(out)
}

fn write_item(out: &mut String, item: &DisplayItem) {
    match item {
        DisplayItem::TextRun(run) => {
            out.push_str(&format!(
                "<div class=\"vutex-text\" data-vutex-item=\"text\" style=\"left:{}pt;top:{}pt;font-size:{}pt;font-family:{}{}\">",
                num(run.x),
                num(run.y - 0.8 * run.size),
                num(run.size),
                escape_attribute(&run.font_family),
                style_declarations(run.style),
            ));
            write_snapshot_text(out, run);
            out.push_str("</div>\n");
        }
        DisplayItem::Rule(rule) => {
            let (left, top, width, height) = if (rule.y2 - rule.y1).abs() <= f64::EPSILON {
                let left = rule.x1.min(rule.x2);
                (left, rule.y1 - rule.thickness / 2.0, (rule.x2 - rule.x1).abs(), rule.thickness)
            }
            else {
                (rule.x1 - rule.thickness / 2.0, rule.y1.min(rule.y2), rule.thickness, (rule.y2 - rule.y1).abs())
            };
            out.push_str(&format!(
                "<div class=\"vutex-rule\" data-vutex-item=\"rule\" style=\"left:{}pt;top:{}pt;width:{}pt;height:{}pt\"></div>\n",
                num(left),
                num(top),
                num(width),
                num(height),
            ));
        }
        DisplayItem::Rect(rect) => {
            let paint = if rect.filled { "background:#f2f2f2".to_string() } else { "border:0.5pt solid #999".to_string() };
            out.push_str(&format!(
                "<div class=\"vutex-rect\" data-vutex-item=\"rect\" style=\"left:{}pt;top:{}pt;width:{}pt;height:{}pt;{}\"></div>\n",
                num(rect.x),
                num(rect.y),
                num(rect.width),
                num(rect.height),
                paint,
            ));
        }
        DisplayItem::Image(image) => {
            out.push_str(&format!(
                "<div class=\"vutex-image\" data-vutex-item=\"image\" role=\"img\" aria-label=\"{}\" style=\"left:{}pt;top:{}pt;width:{}pt;height:{}pt\">{}</div>\n",
                escape_attribute(&image.alt),
                num(image.x),
                num(image.y),
                num(image.width),
                num(image.height),
                escape_text(&image.alt),
            ));
        }
        DisplayItem::Graphic(graphic) => {
            // Snapshot HTML cannot paint a vector scene in page coordinates, so
            // it reserves the same box and keeps the alternative text reachable.
            out.push_str(&format!(
                "<div class=\"vutex-graphic\" data-vutex-item=\"graphic\" role=\"img\" aria-label=\"{}\" style=\"left:{}pt;top:{}pt;width:{}pt;height:{}pt\">{}</div>\n",
                escape_attribute(&graphic.alt),
                num(graphic.x),
                num(graphic.y),
                num(graphic.width),
                num(graphic.height),
                escape_text(&graphic.alt),
            ));
        }
        DisplayItem::Path(path) => write_path(out, path),
    }
}

/// Write the text of a fixed snapshot run, wrapping it in its anchor.
///
/// The anchor sits inside the positioned text element so snapshot geometry is
/// unchanged. A destination that fails [`is_safe_href`] is dropped and the text
/// still renders as plain content; the link title is only written onto an
/// anchor that exists.
fn write_snapshot_text(out: &mut String, run: &DisplayTextRun) {
    let anchor = run.href.as_deref().filter(|href| is_safe_href(href));
    if let Some(href) = anchor {
        write_anchor_open(out, href, run.title.as_deref());
    }
    out.push_str(&escape_text(&run.text));
    if anchor.is_some() {
        out.push_str("</a>");
    }
}

fn write_flow_block(out: &mut String, block: &FlowBlock) {
    match block {
        FlowBlock::Heading { level, runs } => {
            let level = (*level).clamp(1, 6);
            out.push_str(&format!("<h{level} data-vutex-block=\"heading\">"));
            for run in runs {
                write_flow_run(out, run);
            }
            out.push_str(&format!("</h{level}>\n"));
        }
        FlowBlock::Paragraph { runs } => {
            out.push_str("<p data-vutex-block=\"paragraph\">");
            for run in runs {
                write_flow_run(out, run);
            }
            out.push_str("</p>\n");
        }
        FlowBlock::List { ordered, items } => {
            let tag = if *ordered { "ol" } else { "ul" };
            out.push_str(&format!("<{tag} data-vutex-block=\"list\""));
            // The first authored number becomes the list start, so later items
            // only need a `value` where the authored run breaks.
            let first_number = items.first().and_then(|item| item.number);
            if *ordered
                && let Some(number) = first_number
                && number != 1
            {
                out.push_str(&format!(" start=\"{number}\""));
            }
            out.push_str(">\n");
            let mut count = first_number.unwrap_or(1);
            for item in items {
                out.push_str("<li");
                if *ordered {
                    if let Some(number) = item.number {
                        if number != count {
                            out.push_str(&format!(" value=\"{number}\""));
                        }
                        count = number.saturating_add(1);
                    }
                    else {
                        count = count.saturating_add(1);
                    }
                }
                out.push('>');
                for run in &item.runs {
                    write_flow_run(out, run);
                }
                for child in &item.children {
                    write_flow_block(out, child);
                }
                out.push_str("</li>\n");
            }
            out.push_str(&format!("</{tag}>\n"));
        }
        FlowBlock::CodeBlock { language, text } => {
            out.push_str("<pre data-vutex-block=\"code\"><code");
            if let Some(language) = language {
                out.push_str(&format!(" class=\"language-{}\"", escape_attribute(language)));
            }
            out.push_str(&format!(">{}</code></pre>\n", escape_text(text)));
        }
        FlowBlock::Quote { runs } => {
            out.push_str("<blockquote data-vutex-block=\"quote\"><p>");
            for run in runs {
                write_flow_run(out, run);
            }
            out.push_str("</p></blockquote>\n");
        }
        FlowBlock::Table { caption, rows } => {
            out.push_str("<table data-vutex-block=\"table\">\n");
            if let Some(caption) = caption {
                out.push_str(&format!("<caption>{}</caption>\n", escape_text(caption)));
            }
            write_table_rows(out, "thead", rows.iter().filter(|row| row.is_header));
            write_table_rows(out, "tbody", rows.iter().filter(|row| !row.is_header));
            out.push_str("</table>\n");
        }
        FlowBlock::Image { alt, source } => {
            out.push_str(&format!(
                "<figure data-vutex-block=\"image\"><img src=\"{}\" alt=\"{}\">",
                escape_attribute(source),
                escape_attribute(alt),
            ));
            if !alt.is_empty() {
                out.push_str(&format!("<figcaption>{}</figcaption>", escape_text(alt)));
            }
            out.push_str("</figure>\n");
        }
        FlowBlock::Rule => out.push_str("<hr data-vutex-block=\"rule\">\n"),
    }
}

/// Write one flow run, wrapping it in the element that carries its emphasis.
///
/// A run destination nests outside the emphasis element, so authored bold text
/// inside a link renders as `<a href="…"><strong>…</strong></a>`. Runs without a
/// destination keep the plain output, and the link title only appears on an
/// anchor that exists.
fn write_flow_run(out: &mut String, run: &FlowRun) {
    let anchor = run.href.as_deref().filter(|href| is_safe_href(href));
    if let Some(href) = anchor {
        write_anchor_open(out, href, run.title.as_deref());
    }
    let tag = match run.style {
        FontStyle::Bold => Some("strong"),
        FontStyle::Italic => Some("em"),
        FontStyle::Monospace => Some("code"),
        FontStyle::Strike => Some("del"),
        _ => None,
    };
    match tag {
        Some(tag) => out.push_str(&format!("<{tag}>{}</{tag}>", escape_text(&run.text))),
        None => out.push_str(&escape_text(&run.text)),
    }
    if anchor.is_some() {
        out.push_str("</a>");
    }
}

/// Open an anchor, adding the escaped tooltip only when the run carries one.
fn write_anchor_open(out: &mut String, href: &str, title: Option<&str>) {
    out.push_str(&format!("<a href=\"{}\"", escape_attribute(href)));
    if let Some(title) = title {
        out.push_str(&format!(" title=\"{}\"", escape_attribute(title)));
    }
    out.push('>');
}

/// Inline CSS declarations for a styled run inside the fixed snapshot layout.
///
/// Snapshot geometry stays untouched; emphasis is layered on top so the
/// absolute positioning is byte-identical to the unstyled output.
fn style_declarations(style: FontStyle) -> &'static str {
    match style {
        FontStyle::Bold => ";font-weight:700",
        FontStyle::Italic => ";font-style:italic",
        _ => "",
    }
}

/// Render one vector glyph outline as an absolutely-positioned inline `<svg>`.
///
/// The path origin sits on the text baseline with y growing downward, so the
/// inline viewport is placed `height` points above the baseline (a negative
/// viewBox origin) and reaches `depth` points below it. The `<path>` `d`
/// attribute is copied verbatim; browsers natively understand SVG path
/// operators so no conversion is needed.
fn write_path(out: &mut String, path: &DisplayPath) {
    if path.d.is_empty() {
        return;
    }
    let vh = path.height + path.depth;
    out.push_str(&format!(
        "<svg class=\"vutex-path\" data-vutex-item=\"path\" style=\"position:absolute;left:{}pt;top:{}pt;width:{}pt;height:{}pt\" viewBox=\"0 {} {} {}\"><path d=\"{}\" fill=\"#111111\"/></svg>\n",
        num(path.x),
        num(path.y - path.height),
        num(path.width.max(0.001)),
        num(vh.max(0.001)),
        num(-path.height),
        num(path.width.max(0.001)),
        num(vh.max(0.001)),
        escape_attribute(&path.d),
    ));
}

fn write_table_rows<'a>(out: &mut String, section: &str, rows: impl Iterator<Item = &'a FlowTableRow>) {
    let mut wrote_section = false;
    for row in rows {
        if !wrote_section {
            out.push_str(&format!("<{section}>\n"));
            wrote_section = true;
        }
        out.push_str("<tr>");
        let cell_tag = if row.is_header { "th" } else { "td" };
        for cell in &row.cells {
            out.push_str(&format!("<{cell_tag}>"));
            for run in cell {
                write_flow_run(out, run);
            }
            out.push_str(&format!("</{cell_tag}>"));
        }
        out.push_str("</tr>\n");
    }
    if wrote_section {
        out.push_str(&format!("</{section}>\n"));
    }
}

fn is_valid_tag(tag: &str) -> bool {
    !tag.is_empty() && tag.chars().all(|ch| ch.is_ascii_alphanumeric())
}

fn num(value: f64) -> String {
    format!("{value:.3}")
}

fn escape_text(value: &str) -> String {
    value.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn escape_attribute(value: &str) -> String {
    escape_text(value).replace('"', "&quot;").replace('\'', "&#39;")
}
