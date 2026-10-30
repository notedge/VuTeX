//! Render a [`FormulaLayoutResult`] into SVG by walking its [`PaintNode`] tree.
//!
//! Every backend consumes the same immutable [`FormulaLayoutResult`]; this
//! module is the SVG side of that contract.  The paint tree already carries
//! the canvas-space geometry the box-tree renderer used to compute while
//! walking, so the traversal here only maps primitives onto SVG elements and
//! tracks the painted ink for the viewport.

use std::fmt::Write;

use vutex_types::{
    Diagnostic, Severity,
    math::{Fixed, FormulaLayoutResult, PaintNode},
};

/// Output of [`render_formula_result`]: the SVG document plus the warnings
/// collected while walking the paint tree (glyphs the face ships no outline
/// for).
#[derive(Debug, Clone)]
pub struct FormulaSvg {
    pub markup: String,
    pub diagnostics: Vec<Diagnostic>,
}

/// Render a fully laid-out formula as an SVG document.
///
/// The canvas is the formula box from [`FormulaLayoutResult::metrics`]:
/// origin at the box top-left, baseline at `y = metrics.height`.  The viewport
/// is the union of that box and the ink actually painted, padded slightly so
/// scaled outlines are never clipped.
///
/// When `text_fallback` is false (the deterministic default), a glyph the face
/// cannot outline fails the render with an error instead of pulling in a host
/// font through `<text>`; when true, such a glyph paints a host-font text run
/// and reports a `vutex.layout.math.outline_missing` warning.
pub fn render_formula_result(result: &FormulaLayoutResult, text_fallback: bool) -> Result<FormulaSvg, String> {
    let metrics = &result.metrics;
    let mut walk = SvgWalk { buffer: String::new(), ink: None, diagnostics: Vec::new(), text_fallback };
    walk.walk(&result.paint, 0.0, 0.0)?;
    let width = metrics.width.to_f64();
    let height = (metrics.height + metrics.depth).to_f64();
    let diagnostics = std::mem::take(&mut walk.diagnostics);
    Ok(FormulaSvg { markup: walk.finish(width, height), diagnostics })
}

/// Axis-aligned ink bounds in canvas pixels, with y growing downward.
#[derive(Debug, Clone, Copy)]
struct Bounds {
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
}

impl Bounds {
    fn include_point(&mut self, x: f64, y: f64) {
        if !x.is_finite() || !y.is_finite() {
            return;
        }
        self.min_x = self.min_x.min(x);
        self.min_y = self.min_y.min(y);
        self.max_x = self.max_x.max(x);
        self.max_y = self.max_y.max(y);
    }

    fn include_bounds(&mut self, other: &Bounds) {
        self.include_point(other.min_x, other.min_y);
        self.include_point(other.max_x, other.max_y);
    }
}

struct SvgWalk {
    buffer: String,
    /// Bounds of everything painted so far, in canvas pixels.
    ink: Option<Bounds>,
    diagnostics: Vec<Diagnostic>,
    text_fallback: bool,
}

impl SvgWalk {
    fn walk(&mut self, node: &PaintNode, ox: f64, oy: f64) -> Result<(), String> {
        match node {
            PaintNode::Group { children, translate_x, translate_y } => {
                let nx = ox + translate_x.to_f64();
                let ny = oy + translate_y.to_f64();
                let _ = write!(self.buffer, r#"<g transform="translate({}, {})">"#, nx, ny);
                for child in children {
                    self.walk(child, nx, ny)?;
                }
                self.buffer.push_str("</g>");
            }
            PaintNode::GlyphRun { glyph_ids: _, chars, family, positions, size, path_data, font_id: _ } => {
                let scale = size.to_f64();
                for (i, d_opt) in path_data.iter().enumerate() {
                    let (px, py) = positions.get(i).copied().unwrap_or((Fixed::ZERO, Fixed::ZERO));
                    let (rx, ry) = (px.to_f64(), py.to_f64());
                    if let Some(d) = d_opt {
                        self.path(d, ox + rx, oy + ry, rx, ry, scale);
                        continue;
                    }
                    // No outline: the entry is reported whether or not it
                    // paints, matching the box-tree collector this replaces.
                    self.diagnostics.push(outline_missing(chars.get(i).copied().flatten(), family));
                    let text = match chars.get(i).copied().flatten() {
                        Some(c) if !c.is_whitespace() => c.to_string(),
                        _ => continue,
                    };
                    if !self.text_fallback {
                        let msg = format!(
                            "deterministic SVG requires pre-computed glyph paths; \
                             <text> fallback triggered for text={:?} at ({}, {})",
                            text, rx, ry
                        );
                        return Err(msg);
                    }
                    self.text(&text, ox + rx, oy + ry, rx, ry, scale, family);
                }
            }
            PaintNode::Rect { x, y, width, height, stroke_width } => {
                let rx = x.to_f64();
                let ry = y.to_f64();
                let rw = width.to_f64();
                let rh = height.to_f64();
                let rect = Bounds { min_x: ox + rx, min_y: oy + ry, max_x: ox + rx + rw, max_y: oy + ry + rh };
                self.ink.get_or_insert(rect).include_bounds(&rect);
                if stroke_width.to_f64() > 0.0 {
                    let _ = write!(
                        self.buffer,
                        r#"<rect x="{}" y="{}" width="{}" height="{}" fill="currentColor" stroke="currentColor" stroke-width="{}" />"#,
                        rx,
                        ry,
                        rw,
                        rh,
                        stroke_width.to_f64(),
                    );
                }
                else {
                    let _ = write!(
                        self.buffer,
                        r#"<rect x="{}" y="{}" width="{}" height="{}" fill="currentColor" />"#,
                        rx, ry, rw, rh
                    );
                }
            }
            PaintNode::VectorPath { d, x, y, scale, bbox: _bbox } => {
                let rx = x.to_f64();
                let ry = y.to_f64();
                let s = scale.to_f64();
                self.path(d, ox + rx, oy + ry, rx, ry, s);
            }
        }
        Ok(())
    }

    /// Emit one outline path under `translate(x, y) scale(s, -s)`, mapping the
    /// y-up unit space onto the y-down canvas.  `(ax, ay)` is the absolute
    /// anchor for ink tracking, `(rx, ry)` the group-relative one for markup.
    fn path(&mut self, d: &str, ax: f64, ay: f64, rx: f64, ry: f64, scale: f64) {
        let mut ink = self.ink;
        visit_path_points(d, |px, py| {
            let wx = ax + scale * px;
            let wy = ay - scale * py;
            let bounds = ink.get_or_insert(Bounds { min_x: wx, min_y: wy, max_x: wx, max_y: wy });
            bounds.include_point(wx, wy);
        });
        self.ink = ink;
        let _ = write!(
            self.buffer,
            r#"<path d="{}" transform="translate({}, {}) scale({}, {})" fill="currentColor" />"#,
            d,
            num(rx),
            num(ry),
            num(scale),
            num(-scale),
        );
    }

    /// Emit one host-font text run (non-deterministic fallback).
    fn text(&mut self, text: &str, ax: f64, ay: f64, rx: f64, ry: f64, font_size: f64, family: &str) {
        // Rough advance estimate: the backend only needs it to keep the
        // viewport wide enough for the host-font fallback.
        let estimate = font_size * 0.6 * text.chars().count() as f64;
        let box_bounds = Bounds { min_x: ax, min_y: ay - font_size, max_x: ax + estimate, max_y: ay + font_size * 0.25 };
        self.ink.get_or_insert(box_bounds).include_bounds(&box_bounds);
        let font_family_attr = if family == "default" { String::new() } else { format!(r#" font-family="{}""#, family) };
        let _ = write!(
            self.buffer,
            r#"<text x="{}" y="{}" font-size="{}"{} fill="currentColor">{}</text>"#,
            rx, ry, font_size, font_family_attr, text,
        );
    }

    /// Close the document, widening the viewport to cover the painted ink.
    fn finish(self, width: f64, height: f64) -> String {
        let mut bounds = Bounds { min_x: 0.0, min_y: 0.0, max_x: width, max_y: height };
        if let Some(ink) = self.ink {
            bounds.include_bounds(&ink);
        }
        let span = (bounds.max_x - bounds.min_x).max(bounds.max_y - bounds.min_y).max(1.0);
        let pad = (span * 0.04).max(0.5);
        let min_x = bounds.min_x - pad;
        let min_y = bounds.min_y - pad;
        let view_w = (bounds.max_x - bounds.min_x) + 2.0 * pad;
        let view_h = (bounds.max_y - bounds.min_y) + 2.0 * pad;
        let mut svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}" viewBox="{} {} {} {}">"#,
            num(view_w),
            num(view_h),
            num(min_x),
            num(min_y),
            num(view_w),
            num(view_h),
        );
        svg.push_str(&self.buffer);
        svg.push_str("</svg>");
        svg
    }
}

/// Structure diagnostic for a glyph the math font ships no outline for.
fn outline_missing(ch: Option<char>, family: &str) -> Diagnostic {
    let name = match ch {
        Some(ch) => format!("'{ch}' (U+{:04X})", ch as u32),
        None => "without a character".to_string(),
    };
    Diagnostic {
        code: "vutex.layout.math.outline_missing".into(),
        severity: Severity::Warning,
        message: format!(
            "math glyph {name} in family `{family}` has no embedded outline and was not painted; the box keeps its measured advance rather than falling back to a host font"
        ),
    }
}

/// Format a canvas coordinate for the SVG output.
fn num(value: f64) -> String {
    format!("{value:.4}")
}

/// Visit every on-curve/off-curve point of an absolute SVG path.
///
/// Layout only emits absolute `M`, `L`, `Q`, `C` and `Z`. Control points are
/// visited too, which keeps the tracked ink bounds a superset of the outline.
fn visit_path_points(d: &str, mut visit: impl FnMut(f64, f64)) {
    let mut op = 'M';
    let mut args: Vec<f64> = Vec::new();
    let mut chars = d.chars().peekable();
    loop {
        let Some(c) = chars.peek().copied()
        else {
            break;
        };
        if c.is_ascii_alphabetic() {
            emit_points(op, &args, &mut visit);
            args.clear();
            op = c;
            chars.next();
        }
        else if c.is_ascii_digit() || c == '-' || c == '+' || c == '.' {
            let mut token = String::new();
            if c == '-' || c == '+' {
                token.push(c);
                chars.next();
            }
            while let Some(&next) = chars.peek() {
                let exponent_sign = matches!(next, '-' | '+') && matches!(token.chars().last(), Some('e' | 'E'));
                if next.is_ascii_digit() || next == '.' || next == 'e' || next == 'E' || exponent_sign {
                    token.push(next);
                    chars.next();
                }
                else {
                    break;
                }
            }
            if let Ok(value) = token.parse::<f64>() {
                args.push(value);
            }
        }
        else {
            chars.next();
        }
    }
    emit_points(op, &args, &mut visit);
}

/// Feed one operator's coordinate pairs to `visit`.
fn emit_points(op: char, args: &[f64], visit: &mut impl FnMut(f64, f64)) {
    let arity = match op {
        'M' | 'L' | 'T' => 2,
        'Q' | 'S' => 4,
        'C' => 6,
        'A' => 7,
        _ => return,
    };
    for chunk in args.chunks_exact(arity) {
        match op {
            'M' | 'L' | 'T' => visit(chunk[0], chunk[1]),
            'Q' | 'S' => {
                visit(chunk[0], chunk[1]);
                visit(chunk[2], chunk[3]);
            }
            'C' => {
                visit(chunk[0], chunk[1]);
                visit(chunk[2], chunk[3]);
                visit(chunk[4], chunk[5]);
            }
            'A' => visit(chunk[5], chunk[6]),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vutex_types::math::{
        FormulaMetrics, MathSemantics,
        formula_layout::{BBox, FontMetricsSnapshot},
    };

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
            paint: PaintNode::Group {
                children: vec![PaintNode::Rect {
                    x: Fixed::from_f64(10.0),
                    y: Fixed::ZERO,
                    width: Fixed::from_f64(80.0),
                    height: Fixed::from_f64(15.0),
                    stroke_width: Fixed::ZERO,
                }],
                translate_x: Fixed::ZERO,
                translate_y: Fixed::ZERO,
            },
            diagnostics: Vec::new(),
            fonts: FontMetricsSnapshot::default(),
            semantics: MathSemantics::default(),
        }
    }

    #[test]
    fn rect_paint_emits_svg_rect() {
        let svg = render_formula_result(&rect_result(), false).expect("render").markup;
        assert!(svg.starts_with(r#"<svg xmlns="http://www.w3.org/2000/svg""#));
        assert!(svg.contains("<rect "));
        assert!(svg.contains("width="));
        assert!(svg.ends_with("</svg>"));
    }

    #[test]
    fn group_translate_offsets_children() {
        let mut res = rect_result();
        res.paint = PaintNode::Group {
            children: vec![PaintNode::Group {
                children: vec![PaintNode::Rect {
                    x: Fixed::from_f64(1.0),
                    y: Fixed::from_f64(2.0),
                    width: Fixed::from_f64(3.0),
                    height: Fixed::from_f64(4.0),
                    stroke_width: Fixed::ZERO,
                }],
                translate_x: Fixed::from_f64(10.0),
                translate_y: Fixed::from_f64(20.0),
            }],
            translate_x: Fixed::ZERO,
            translate_y: Fixed::ZERO,
        };
        let svg = render_formula_result(&res, false).expect("render").markup;
        assert!(svg.contains(r#"<g transform="translate(10, 20)">"#), "{svg}");
        assert!(svg.contains(r#"<rect x="1" y="2" width="3" height="4"#), "{svg}");
    }

    #[test]
    fn vector_path_paint_emits_svg_path() {
        let mut res = rect_result();
        res.paint = PaintNode::Group {
            children: vec![PaintNode::VectorPath {
                d: "M 0 0 L 10 0 L 5 10 Z".to_string(),
                x: Fixed::from_f64(1.0),
                y: Fixed::from_f64(2.0),
                scale: Fixed::ONE,
                bbox: BBox { x: Fixed::ZERO, y: Fixed::ZERO, width: Fixed::from_f64(10.0), height: Fixed::from_f64(10.0) },
            }],
            translate_x: Fixed::ZERO,
            translate_y: Fixed::ZERO,
        };
        let svg = render_formula_result(&res, false).expect("render").markup;
        assert!(svg.contains("<path d=\"M 0 0 L 10 0 L 5 10 Z\""), "{svg}");
        assert!(svg.contains("scale(1.0000, -1.0000)"), "{svg}");
    }

    #[test]
    fn empty_paint_still_emits_valid_svg() {
        let res = FormulaLayoutResult::default();
        let svg = render_formula_result(&res, false).expect("render").markup;
        assert!(svg.starts_with("<svg"));
        assert!(svg.ends_with("</svg>"));
    }

    #[test]
    fn glyph_run_outlines_paint_paths() {
        let mut res = rect_result();
        res.paint = PaintNode::Group {
            children: vec![PaintNode::GlyphRun {
                font_id: "lmr".to_string(),
                glyph_ids: vec![65],
                chars: vec![Some('A')],
                family: "latin-modern-math".to_string(),
                positions: vec![(Fixed::from_f64(3.0), Fixed::from_f64(4.0))],
                size: Fixed::from_f64(16.0),
                path_data: vec![Some("M 0 0 L 1 1 Z".to_string())],
            }],
            translate_x: Fixed::ZERO,
            translate_y: Fixed::ZERO,
        };
        let svg = render_formula_result(&res, false).expect("render").markup;
        assert!(
            svg.contains(r#"<path d="M 0 0 L 1 1 Z" transform="translate(3.0000, 4.0000) scale(16.0000, -16.0000)""#),
            "{svg}"
        );
    }

    #[test]
    fn glyph_run_without_outline_fails_strictly() {
        let mut res = rect_result();
        res.paint = PaintNode::Group {
            children: vec![PaintNode::GlyphRun {
                font_id: "lmr".to_string(),
                glyph_ids: vec![0],
                chars: vec![Some('字')],
                family: "latin-modern-math".to_string(),
                positions: vec![(Fixed::from_f64(3.0), Fixed::from_f64(4.0))],
                size: Fixed::from_f64(16.0),
                path_data: vec![None],
            }],
            translate_x: Fixed::ZERO,
            translate_y: Fixed::ZERO,
        };
        let err = render_formula_result(&res, false).expect_err("strict mode must reject a host-font run");
        assert!(err.contains("deterministic SVG requires pre-computed glyph paths"), "{err}");
    }

    #[test]
    fn glyph_run_without_outline_falls_back_to_text() {
        let mut res = rect_result();
        res.paint = PaintNode::Group {
            children: vec![PaintNode::GlyphRun {
                font_id: "lmr".to_string(),
                glyph_ids: vec![0],
                chars: vec![Some('字')],
                family: "latin-modern-math".to_string(),
                positions: vec![(Fixed::from_f64(3.0), Fixed::from_f64(4.0))],
                size: Fixed::from_f64(16.0),
                path_data: vec![None],
            }],
            translate_x: Fixed::ZERO,
            translate_y: Fixed::ZERO,
        };
        let out = render_formula_result(&res, true).expect("fallback mode renders");
        assert!(out.markup.contains("<text x=\"3\" y=\"4\""), "{}", out.markup);
        assert_eq!(out.diagnostics.len(), 1);
        assert_eq!(out.diagnostics[0].code, "vutex.layout.math.outline_missing");
    }
}
