//! HTML backend for formula rendering.
//!
//! Paints the laid-out formula result as absolutely positioned CSS boxes
//! inside one inline-block container — the same flow model KaTeX uses, so the
//! formula aligns with surrounding text through `vertical-align` and scales
//! with the page. Glyphs the selected face reaches through its character map
//! are emitted as real text (selectable, accessible, no embedded outlines);
//! variant-selected or assembled glyphs that text cannot express fall back to
//! tiny inline SVG fragments, so no ink is ever dropped.
//!
//! The traversal walks the [`PaintNode`] tree from the single
//! [`FormulaLayoutResult`], the same canvas-space geometry the SVG and PDF
//! backends emit. The text/path choice per glyph needs the glyph's
//! `char`/`glyph_id` pair, which the paint tree carries directly.

use std::collections::HashMap;

use acorn_ttf::FontFace;
use vutex_text::math::MathFontFamily;
use vutex_types::math::{Fixed, FormulaLayoutResult, PaintNode};

/// Per-face data the HTML backend reads from an embedded math font.
#[derive(Debug)]
struct FaceData {
    /// Offset from the top of a zero-line-height box down to the text
    /// baseline, per em pixel: `(hhea.ascender + hhea.descender) / (2 * upem)`.
    baseline: f64,
    /// Lazily-parsed face used to answer cmap questions.
    face: FontFace,
}

impl FaceData {
    /// Parse one embedded family, or `None` when it ships no asset.
    fn load(family: MathFontFamily) -> Option<Self> {
        let bytes = family.bytes()?;
        let mut face = FontFace::load(bytes).ok()?;
        let (ascent, descent) = face.hhea().map(|hhea| (hhea.ascent, hhea.descent))?;
        let upem = face.units_per_em().max(1) as f64;
        Some(Self { baseline: (i32::from(ascent) + i32::from(descent)) as f64 / (2.0 * upem), face })
    }

    /// The glyph the font's character map assigns to `ch`.
    fn cmap_glyph(&mut self, ch: char) -> Option<u16> {
        self.face.glyph_index(ch)
    }
}

/// Resolve a layout font-family label onto an embedded family.
///
/// `"default"` is the family the layout engine resolved for this render, so
/// the caller passes the selected face as the fallback.
fn embedded_family_for(label: &str, fallback: MathFontFamily) -> Option<MathFontFamily> {
    match label {
        "xits-math" => Some(MathFontFamily::XitsMath),
        "latin-modern-math" => Some(MathFontFamily::LmMath),
        "default" => Some(fallback),
        _ => None,
    }
}

/// Renders a [`FormulaLayoutResult`] paint tree into an HTML fragment.
#[derive(Debug)]
pub struct HtmlBackend {
    buffer: String,
    /// The face the host selected for this render; `"default"` labels resolve
    /// against it, and its label is the CSS family registered by `@font-face`.
    fallback_family: MathFontFamily,
    /// Parsed faces keyed by family label, filled lazily per render.
    faces: HashMap<&'static str, Option<FaceData>>,
    /// Open group spans awaiting their closing tag.
    group_depth: usize,
}

impl HtmlBackend {
    /// Create a backend whose unlabeled glyphs render in `family`.
    pub fn new(family: MathFontFamily) -> Self {
        Self { buffer: String::new(), fallback_family: family, faces: HashMap::new(), group_depth: 0 }
    }

    /// Close the walk and return the complete container markup.
    ///
    /// The container is sized by the result metrics: `height` above the
    /// baseline, `depth` below, with `vertical-align` dropping the baseline
    /// into the host text line exactly like a text glyph would. The paint
    /// tree's canvas origin is the box top-left, so the walk starts at
    /// `(0, 0)` and the root group already carries the baseline offset.
    pub fn render(mut self, result: &FormulaLayoutResult) -> String {
        let metrics = &result.metrics;
        let width = metrics.width.to_f64();
        let height = metrics.height.to_f64();
        let depth = metrics.depth.to_f64();
        use std::fmt::Write;
        let _ = write!(
            self.buffer,
            r#"<span class="wetex-html" style="position:relative;display:inline-block;width:{}px;height:{}px;vertical-align:-{}px">"#,
            num(width),
            num(height + depth),
            num(depth)
        );
        self.walk(&result.paint, 0.0, 0.0);
        let mut buffer = self.buffer;
        for _ in 0..self.group_depth {
            buffer.push_str("</span>");
        }
        buffer.push_str("</span>");
        buffer
    }

    /// Recursively paint one paint node at the given canvas offset.
    fn walk(&mut self, node: &PaintNode, ox: f64, oy: f64) {
        match node {
            PaintNode::Group { children, translate_x, translate_y } => {
                let nx = ox + translate_x.to_f64();
                let ny = oy + translate_y.to_f64();
                self.start_group(nx, ny);
                for child in children {
                    self.walk(child, nx, ny);
                }
                self.end_group();
            }
            PaintNode::GlyphRun { glyph_ids, chars, family, positions, size, path_data, .. } => {
                let size = size.to_f64();
                for (i, d_opt) in path_data.iter().enumerate() {
                    let (px, py) = positions.get(i).copied().unwrap_or((Fixed::ZERO, Fixed::ZERO));
                    let (x, y) = (ox + px.to_f64(), oy + py.to_f64());
                    let text = match chars.get(i).copied().flatten() {
                        Some(c) if !c.is_whitespace() => Some(c),
                        _ => None,
                    };
                    if let Some(c) = text {
                        if self.text_faithful(c, glyph_ids.get(i).copied(), family) {
                            self.text(c, x, y, size, family);
                            continue;
                        }
                    }
                    if let Some(d) = d_opt {
                        self.path(d, x, y, size);
                    }
                    else if let Some(c) = text {
                        // A text-only key with no outline still has to paint.
                        self.text(c, x, y, size, family);
                    }
                }
            }
            PaintNode::Rect { x, y, width, height, .. } => {
                self.rect(ox + x.to_f64(), oy + y.to_f64(), width.to_f64(), height.to_f64());
            }
            PaintNode::VectorPath { d, x, y, scale, .. } => {
                self.path(d, ox + x.to_f64(), oy + y.to_f64(), scale.to_f64());
            }
        }
    }

    /// Whether emitting `ch` as text reproduces the glyph the layout chose.
    ///
    /// Char-only keys (no resolved id) always do. Variant-selected keys (large
    /// operators, stretched delimiters) only when the cmap entry for the char
    /// is that very glyph — otherwise the text run would paint the wrong
    /// outline.
    fn text_faithful(&mut self, ch: char, glyph_id: Option<u16>, label: &str) -> bool {
        let Some(family) = embedded_family_for(label, self.fallback_family)
        else {
            return false;
        };
        let Some(data) = self.face_mut(family)
        else {
            return false;
        };
        match glyph_id {
            None => true,
            Some(id) => data.cmap_glyph(ch) == Some(id),
        }
    }

    /// The parsed face for `family`, loaded once per backend.
    fn face_mut(&mut self, family: MathFontFamily) -> Option<&mut FaceData> {
        let label = family.label();
        if !self.faces.contains_key(label) {
            self.faces.insert(label, FaceData::load(family));
        }
        self.faces.get_mut(label).and_then(Option::as_mut)
    }

    /// The baseline offset ratio for the face `label` resolves to.
    fn baseline_ratio(&mut self, label: &str) -> Option<f64> {
        let family = embedded_family_for(label, self.fallback_family)?;
        self.face_mut(family).map(|data| data.baseline)
    }

    /// Emit one absolutely positioned text run.
    fn text(&mut self, ch: char, x: f64, y: f64, size: f64, label: &str) {
        let css_family = embedded_family_for(label, self.fallback_family).map(MathFontFamily::label).unwrap_or(label);
        let ratio = self.baseline_ratio(label).unwrap_or(0.0);
        use std::fmt::Write;
        let _ = write!(
            self.buffer,
            r#"<span style="position:absolute;left:{}px;top:{}px;font-size:{}px;line-height:0;white-space:pre;font-family:'{}'">{}</span>"#,
            num(x),
            num(y - ratio * size),
            num(size),
            css_family,
            escape(ch)
        );
    }

    /// Emit one filled rule rectangle.
    fn rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        use std::fmt::Write;
        let _ = write!(
            self.buffer,
            r#"<span style="position:absolute;left:{}px;top:{}px;width:{}px;height:{}px;background:currentColor"></span>"#,
            num(x),
            num(y),
            num(w),
            num(h)
        );
    }

    /// Emit one outline path as an inline SVG sized to its own ink.
    ///
    /// Path data arrives in em units with `y` growing upward from the
    /// baseline; `translate(x, y) scale(s, -s)` maps it onto the y-down
    /// canvas. The element's box is the transformed ink bounds (padded), with
    /// a matching `viewBox`, so nothing relies on `overflow: visible` —
    /// Chrome does not paint viewport-external svg content that way.
    fn path(&mut self, d: &str, x: f64, y: f64, scale: f64) {
        let (min_x, min_y, max_x, max_y) = path_bounds(d, x, y, scale);
        let pad = 1.0;
        let left = min_x - pad;
        let top = min_y - pad;
        let width = (max_x - min_x) + 2.0 * pad;
        let height = (max_y - min_y) + 2.0 * pad;
        use std::fmt::Write;
        let _ = write!(
            self.buffer,
            r#"<svg style="position:absolute;left:{}px;top:{}px;width:{}px;height:{}px" viewBox="{} {} {} {}" preserveAspectRatio="none"><path d="{}" transform="translate({},{}) scale({},{})" fill="currentColor"/></svg>"#,
            num(left),
            num(top),
            num(width),
            num(height),
            num(left),
            num(top),
            num(width),
            num(height),
            d,
            num(x),
            num(y),
            num(scale),
            num(-scale)
        );
    }

    /// Open a positioned group span translated to `(x, y)`.
    fn start_group(&mut self, x: f64, y: f64) {
        use std::fmt::Write;
        let _ = write!(
            self.buffer,
            r#"<span style="position:absolute;left:0;top:0;width:0;height:0;transform:translate({}px,{}px)">"#,
            num(x),
            num(y)
        );
        self.group_depth += 1;
    }

    /// Close the innermost group span.
    fn end_group(&mut self) {
        self.buffer.push_str("</span>");
        self.group_depth -= 1;
    }
}

/// Axis-aligned ink bounds of one transformed path, in group-relative pixels.
///
/// Control points are included, so the box is a superset of the outline — the
/// same safe over-approximation the SVG backend uses for its viewport.
fn path_bounds(d: &str, x: f64, y: f64, scale: f64) -> (f64, f64, f64, f64) {
    let mut bounds = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    let mut count = 0usize;
    visit_path_points(d, &mut |px, py| {
        let wx = x + scale * px;
        let wy = y - scale * py;
        bounds.0 = bounds.0.min(wx);
        bounds.1 = bounds.1.min(wy);
        bounds.2 = bounds.2.max(wx);
        bounds.3 = bounds.3.max(wy);
        count += 1;
    });
    if count == 0 { (x, y, x, y) } else { bounds }
}

/// Visit every on-curve/off-curve point of an absolute SVG path.
///
/// Layout only emits absolute `M`, `L`, `Q`, `C` and `Z`.
fn visit_path_points(d: &str, visit: &mut impl FnMut(f64, f64)) {
    let mut op = 'M';
    let mut args: Vec<f64> = Vec::new();
    let mut chars = d.chars().peekable();
    loop {
        let Some(c) = chars.peek().copied()
        else {
            break;
        };
        if c.is_ascii_alphabetic() {
            emit_points(op, &args, visit);
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
    emit_points(op, &args, visit);
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

/// Format a canvas coordinate for the style attribute.
fn num(value: f64) -> String {
    format!("{value:.4}")
}

/// Escape one character for HTML text content.
fn escape(ch: char) -> String {
    match ch {
        '&' => "&amp;".to_string(),
        '<' => "&lt;".to_string(),
        '>' => "&gt;".to_string(),
        _ => ch.to_string(),
    }
}
