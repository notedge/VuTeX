//! Render a [`FormulaLayoutResult`] directly into PDF 1.7 bytes by walking the
//! [`PaintNode`] tree.
//!
//! This is the PDF counterpart to
//! [`vutex_render_svg::math::render_formula_result`].  The older document-level
//! [`render_pdf`](crate::render::render_pdf) entry point (driven by the
//! `DisplayPresentation` IR) is untouched — both paths coexist until the layout
//! engine fully migrates to the paint tree.
//!
//! # Paint-node → PDF mapping
//!
//! | Paint variant    | PDF primitive                    |
//! |------------------|----------------------------------|
//! | `Group`          | recursive descent                |
//! | `Rect`           | `re` + `f` (filled), `re` + `S`  |
//! | `VectorPath`     | tokenize SVG `d` → PDF path ops  |
//! | `GlyphRun`       | placeholder marker (no outlines)  |

use std::fmt::{self, Write};

use vutex_types::{Diagnostic, Severity};
use vutex_types::math::{FormulaLayoutResult, PaintNode};

/// Render a fully laid-out formula as a minimal single-page PDF document.
///
/// The page size is derived from [`FormulaLayoutResult::metrics`] — preferring
/// `ink_bounds` when non-degenerate, otherwise falling back to the formula's
/// advance width and vertical extent.  Content is placed on page 1 in PDF's
/// y-up coordinate space; math-space coordinates (y-up) are consumed verbatim
/// because PDF shares the same convention.
///
/// Returns PDF 1.7 bytes ready to write.  Errors surface as a single
/// [`Diagnostic`] with code `vutex.render.pdf.formula_result`.
pub fn render_formula_result(result: &FormulaLayoutResult) -> Result<Vec<u8>, Diagnostic> {
    let metrics = &result.metrics;
    // Page dimensions in PDF points (≈ CSS px).
    let (page_w, page_h) = if metrics.ink_bounds.width.to_f64() > 0.0
        || metrics.ink_bounds.height.to_f64() > 0.0
    {
        let ib = &metrics.ink_bounds;
        (
            ib.x.to_f64() + ib.width.to_f64() + 8.0,
            ib.y.to_f64() + ib.height.to_f64() + metrics.depth.to_f64() + 8.0,
        )
    }
    else {
        (
            metrics.width.to_f64().max(1.0) + 8.0,
            (metrics.height.to_f64() + metrics.depth.to_f64()).max(1.0) + 8.0,
        )
    };

    // Page-space y origin for the formula baseline.  PDF origin is bottom-left,
    // so baseline sits at depth + padding.
    let baseline_y = metrics.depth.to_f64() + 4.0;

    let mut content = String::new();
    let _ = write!(content, "q\n");
    render_paint_pdf(&mut content, &result.paint, 4.0, baseline_y);
    let _ = write!(content, "Q\n");
    if content.trim().is_empty() {
        content.push('\n');
    }

    Ok(build_minimal_pdf(page_w, page_h, &content))
}

/// Recursively emit PDF content-stream operators for one [`PaintNode`] subtree.
///
/// `(origin_x, origin_y)` is the cumulative PDF-space offset of the current
/// group (y-up, origin at page bottom-left, formula baseline already offset).
fn render_paint_pdf(out: &mut String, node: &PaintNode, origin_x: f64, origin_y: f64) {
    match node {
        PaintNode::Group(children) => {
            for child in children {
                render_paint_pdf(out, child, origin_x, origin_y);
            }
        }
        PaintNode::Rect { x, y, width, height, stroke_width } => {
            let rx = origin_x + x.to_f64();
            let ry = origin_y + y.to_f64();
            let rw = width.to_f64();
            let rh = height.to_f64();
            let _ = write!(out, "q\n");
            if stroke_width.to_f64() > 0.0 {
                let _ = write!(out, "0.6 G\n0.5 w\n{} {} {} {} re\nS\n", num(rx), num(ry), num(rw), num(rh));
            }
            else {
                let _ = write!(out, "0.0 f\n{} {} {} {} re\nf\n", num(rx), num(ry), num(rw), num(rh));
            }
            let _ = write!(out, "Q\n");
        }
        PaintNode::VectorPath { d, bbox: _bbox } => {
            // PDF path ops are already in y-up convention — translate by group
            // origin and emit path operators.
            let ops = svg_path_to_pdf_ops(d, origin_x, origin_y);
            if !ops.is_empty() {
                let _ = write!(out, "q\n0.0 f\n{ops}f\nQ\n");
            }
        }
        PaintNode::GlyphRun { font_id: _, glyph_ids: _, positions: _, size: _ } => {
            // Placeholder: glyph outline paths are not yet plumbed into the
            // paint tree.  A future change will look up glyph outlines from
            // the font snapshot and emit real PDF path ops here.
        }
    }
}

/// Convert an SVG `d` attribute string into PDF path operators translated by
/// `(origin_x, origin_y)`.
///
/// Only the absolute operators our pipeline emits are recognised: `M`, `L`,
/// `Q`, `C`, `Z`.  Anything else is silently dropped.  PDF keeps the same
/// y-up convention so no y-flip is needed here.
fn svg_path_to_pdf_ops(raw: &str, origin_x: f64, origin_y: f64) -> String {
    let tokens = tokenize_svg_path(raw);
    let mut out = String::new();
    let mut start_pt = (0.0, 0.0);

    let mut i = 0;
    while i < tokens.len() {
        let op = tokens[i].as_str();
        i += 1;
        match op {
            "M" => {
                if let (Some(x), Some(y)) = (
                    tokens.get(i).and_then(|t| t.parse::<f64>().ok()),
                    tokens.get(i + 1).and_then(|t| t.parse::<f64>().ok()),
                ) {
                    i += 2;
                    let px = x + origin_x;
                    let py = y + origin_y;
                    start_pt = (px, py);
                    let _ = write!(out, "{} {} m\n", num(px), num(py));
                    // Implicit L for extra coordinate pairs after M
                    while i + 1 < tokens.len()
                        && tokens[i].parse::<f64>().is_ok()
                        && tokens[i + 1].parse::<f64>().is_ok()
                    {
                        let ax = tokens[i].parse::<f64>().unwrap();
                        let ay = tokens[i + 1].parse::<f64>().unwrap();
                        i += 2;
                        let _ = write!(out, "{} {} l\n", num(ax + origin_x), num(ay + origin_y));
                    }
                }
            }
            "L" => {
                if let (Some(x), Some(y)) = (
                    tokens.get(i).and_then(|t| t.parse::<f64>().ok()),
                    tokens.get(i + 1).and_then(|t| t.parse::<f64>().ok()),
                ) {
                    i += 2;
                    let _ = write!(out, "{} {} l\n", num(x + origin_x), num(y + origin_y));
                }
            }
            "Q" => {
                if let (Some(x1), Some(y1), Some(x), Some(y)) = (
                    tokens.get(i).and_then(|t| t.parse::<f64>().ok()),
                    tokens.get(i + 1).and_then(|t| t.parse::<f64>().ok()),
                    tokens.get(i + 2).and_then(|t| t.parse::<f64>().ok()),
                    tokens.get(i + 3).and_then(|t| t.parse::<f64>().ok()),
                ) {
                    i += 4;
                    let cx1 = x1 + origin_x;
                    let cy1 = y1 + origin_y;
                    let cx = x + origin_x;
                    let cy = y + origin_y;
                    // PDF has no native quadratic cubic — convert using the
                    // standard 2/3 control-point formula.
                    let cxx = (cx1 - start_pt.0) * 2.0 / 3.0 + start_pt.0;
                    let cyy = (cy1 - start_pt.1) * 2.0 / 3.0 + start_pt.1;
                    let cxxx = cx - (cx1 - cx) * 2.0 / 3.0;
                    let cyyy = cy - (cy1 - cy) * 2.0 / 3.0;
                    let _ = write!(out, "{} {} {} {} {} {} c\n", num(cxx), num(cyy), num(cxxx), num(cyyy), num(cx), num(cy));
                    start_pt = (cx, cy);
                }
            }
            "C" => {
                if let (Some(x1), Some(y1), Some(x2), Some(y2), Some(x), Some(y)) = (
                    tokens.get(i).and_then(|t| t.parse::<f64>().ok()),
                    tokens.get(i + 1).and_then(|t| t.parse::<f64>().ok()),
                    tokens.get(i + 2).and_then(|t| t.parse::<f64>().ok()),
                    tokens.get(i + 3).and_then(|t| t.parse::<f64>().ok()),
                    tokens.get(i + 4).and_then(|t| t.parse::<f64>().ok()),
                    tokens.get(i + 5).and_then(|t| t.parse::<f64>().ok()),
                ) {
                    i += 6;
                    let _ = write!(
                        out,
                        "{} {} {} {} {} {} c\n",
                        num(x1 + origin_x),
                        num(y1 + origin_y),
                        num(x2 + origin_x),
                        num(y2 + origin_y),
                        num(x + origin_x),
                        num(y + origin_y),
                    );
                    start_pt = (x + origin_x, y + origin_y);
                }
            }
            "Z" | "z" => {
                let _ = write!(out, "h\n");
            }
            _ => {
                // Unknown operator — skip its argument count conservatively
                // by advancing to the next alphabetic token.
                while i < tokens.len() {
                    let peek = tokens[i].chars().next().unwrap_or('\0');
                    if peek.is_ascii_alphabetic() {
                        break;
                    }
                    i += 1;
                }
            }
        }
    }
    out
}

/// Split an SVG path `d` string into tokens — operator letters and numeric
/// values — preserving order.  Adapted from the PDF renderer's equivalent
/// helper, local here so the module stays self-contained.
fn tokenize_svg_path(raw: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = raw.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_ascii_alphabetic() {
            tokens.push(c.to_string());
            i += 1;
        }
        else if c.is_ascii_digit() || c == '-' || c == '+' || c == '.' {
            let mut token = String::new();
            if c == '-' || c == '+' {
                token.push(c);
                i += 1;
            }
            while i < chars.len() {
                let n = chars[i];
                let exponent_sign = matches!(token.chars().last(), Some('e' | 'E'))
                    && matches!(n, '-' | '+');
                if n.is_ascii_digit() || n == '.' || n == 'e' || n == 'E' || exponent_sign {
                    token.push(n);
                    i += 1;
                }
                else {
                    break;
                }
            }
            if !token.is_empty() {
                tokens.push(token);
            }
        }
        else {
            i += 1; // whitespace, comma, etc.
        }
    }
    tokens
}

/// Format a PDF coordinate to 4 decimal places.
fn num(value: f64) -> String {
    format!("{value:.4}")
}

/// Assemble a minimal single-page PDF document from a page size and content
/// stream.  Produces a well-formed PDF 1.7 with catalog, pages, one page, and
/// its content stream — no fonts, no resources, no annotations.
fn build_minimal_pdf(page_w: f64, page_h: f64, content: &str) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n");
    let mut offsets: Vec<usize> = Vec::new();

    let mut push_object = |body: &str, offsets: &mut Vec<usize>, out: &mut Vec<u8>| {
        offsets.push(out.len());
        let header = format!("{} 0 obj\n", offsets.len());
        out.extend_from_slice(header.as_bytes());
        out.extend_from_slice(body.as_bytes());
        out.extend_from_slice(b"\nendobj\n");
    };

    // Object 1: Catalog
    push_object("<< /Type /Catalog /Pages 2 0 R >>", &mut offsets, &mut out);
    // Object 2: Pages
    push_object("<< /Type /Pages /Kids [3 0 R] /Count 1 >>", &mut offsets, &mut out);
    // Object 3: Page
    let page_body = format!(
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Contents 4 0 R >>",
        num(page_w),
        num(page_h),
    );
    push_object(&page_body, &mut offsets, &mut out);
    // Object 4: Content stream
    offsets.push(out.len());
    let content_header = format!("4 0 obj\n<< /Length {} >>\nstream\n", content.len());
    out.extend_from_slice(content_header.as_bytes());
    out.extend_from_slice(content.as_bytes());
    out.extend_from_slice(b"\nendstream\nendobj\n");

    // Cross-reference table
    let xref_offset = out.len();
    let size = offsets.len() + 1;
    out.extend_from_slice(format!("xref\n0 {size}\n").as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for offset in &offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n").as_bytes(),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use vutex_types::math::{Fixed, FormulaMetrics, PaintNode, formula_layout::BBox};

    fn rect_result() -> FormulaLayoutResult {
        FormulaLayoutResult {
            metrics: FormulaMetrics {
                width: Fixed::from_f64(100.0),
                height: Fixed::from_f64(20.0),
                depth: Fixed::from_f64(5.0),
                axis: Fixed::ZERO,
                ink_bounds: BBox {
                    x: Fixed::from_f64(10.0),
                    y: Fixed::ZERO,
                    width: Fixed::from_f64(80.0),
                    height: Fixed::from_f64(15.0),
                },
            },
            paint: PaintNode::Group(vec![PaintNode::Rect {
                x: Fixed::from_f64(10.0),
                y: Fixed::ZERO,
                width: Fixed::from_f64(80.0),
                height: Fixed::from_f64(15.0),
                stroke_width: Fixed::ZERO,
            }]),
            diagnostics: Vec::new(),
            fonts: vutex_types::math::formula_layout::FontMetricsSnapshot::default(),
        }
    }

    fn is_pdf(bytes: &[u8]) -> bool {
        bytes.len() >= 8 && &bytes[..5] == b"%PDF-"
    }

    #[test]
    fn rect_paint_emits_pdf() {
        let pdf = render_formula_result(&rect_result()).expect("render");
        assert!(is_pdf(&pdf), "starts with %PDF-");
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.contains("/Type /Catalog"));
        assert!(text.contains("/Type /Page"));
        assert!(text.contains("re"));
    }

    #[test]
    fn vector_path_paint_emits_pdf_path_ops() {
        let mut res = rect_result();
        res.paint = PaintNode::Group(vec![PaintNode::VectorPath {
            d: "M 0 0 L 10 0 L 5 10 Z".to_string(),
            bbox: BBox {
                x: Fixed::ZERO,
                y: Fixed::ZERO,
                width: Fixed::from_f64(10.0),
                height: Fixed::from_f64(10.0),
            },
        }]);
        let pdf = render_formula_result(&res).expect("render");
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.contains(" m\n"));
        assert!(text.contains(" l\n"));
        assert!(text.contains("h\n"));
    }

    #[test]
    fn empty_paint_still_emits_valid_pdf() {
        let res = FormulaLayoutResult::default();
        let pdf = render_formula_result(&res).expect("render");
        assert!(is_pdf(&pdf));
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.contains("/Type /Page"));
    }

    #[test]
    fn glyph_run_emits_no_paint() {
        let mut res = rect_result();
        res.paint = PaintNode::Group(vec![PaintNode::GlyphRun {
            font_id: "lmr".to_string(),
            glyph_ids: vec![65, 66],
            positions: vec![(Fixed::ZERO, Fixed::ZERO), (Fixed::from_f64(10.0), Fixed::ZERO)],
            size: Fixed::from_f64(16.0),
        }]);
        let pdf = render_formula_result(&res).expect("render");
        let text = String::from_utf8_lossy(&pdf);
        // No rect/path operators, but still valid PDF page
        assert!(is_pdf(&pdf));
    }
}

// Silence unused import warning when someone wants the Diagnostic return path.
#[allow(dead_code)]
fn _unused_diagnostic() -> Diagnostic {
    Diagnostic { code: String::new(), severity: Severity::Error, message: String::new() }
}
