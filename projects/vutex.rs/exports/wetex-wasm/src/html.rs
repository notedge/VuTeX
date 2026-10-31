//! HTML backend for formula rendering.
//!
//! Paints the laid-out formula result as absolutely positioned CSS boxes
//! inside one inline-block container — the same flow model KaTeX uses, so the
//! formula aligns with surrounding text through `vertical-align` and scales
//! with the page.
//!
//! The leaf population is the SVG backend's, one-for-one: every glyph the
//! paint tree carries an outline for becomes one tiny inline SVG fragment,
//! every rule becomes one positioned box, and a glyph the selected face ships
//! no outline for is skipped and named in a `vutex.layout.math.outline_missing`
//! warning, never pulled in through a host-font text run. Both backends walk
//! the same [`PaintNode`] tree from the single [`FormulaLayoutResult`], so the
//! cross-backend geometry gate sees the same leaves at the same composed
//! anchors.
//!
//! # Coordinate space
//!
//! The fragment mirrors the paint tree's nested coordinate space one-to-one,
//! exactly like the SVG backend's nested `<g transform>` markup. Each
//! [`PaintNode::Group`] becomes a positioned span whose `transform` carries
//! **only that group's own `translate_x`/`translate_y`** — the offset from the
//! enclosing group origin, never the accumulated canvas position. Every leaf
//! (rule `rect`, outline `svg` fragment) is positioned with its node-local
//! coordinates, relative to the innermost enclosing group span.
//! CSS containing-block nesting composes the ancestor offsets, so each
//! coordinate is written once and no offset is ever double-counted. The
//! container span is the canvas origin: the root group's transform carries the
//! baseline offset from the box top-left, as the layout emits it.

use vutex_types::{
    Diagnostic, Severity,
    math::{Fixed, FormulaLayoutResult, PaintNode},
};

/// Output of [`HtmlBackend::render`]: the container markup plus the warnings
/// collected while walking the paint tree (glyphs the face ships no outline
/// for), mirroring the SVG backend's report.
#[derive(Debug)]
pub struct FormulaHtml {
    /// The complete container fragment.
    pub markup: String,
    /// One `vutex.layout.math.outline_missing` warning per skipped glyph.
    pub diagnostics: Vec<Diagnostic>,
}

/// Renders a [`FormulaLayoutResult`] paint tree into an HTML fragment.
#[derive(Debug)]
pub struct HtmlBackend {
    buffer: String,
    /// Warnings for glyphs skipped for lack of an embedded outline.
    diagnostics: Vec<Diagnostic>,
    /// Open group spans awaiting their closing tag.
    group_depth: usize,
}

impl HtmlBackend {
    /// Create a backend painting the shared outline-only leaf contract.
    pub fn new() -> Self {
        Self { buffer: String::new(), diagnostics: Vec::new(), group_depth: 0 }
    }

    /// Close the walk and return the complete container markup with the
    /// skipped-glyph warnings.
    ///
    /// The container is sized by the result metrics: `height` above the
    /// baseline, `depth` below, with `vertical-align` dropping the baseline
    /// into the host text line exactly like a text glyph would. The container
    /// is the canvas origin (box top-left); the paint tree's root group
    /// transform carries the baseline offset from it.
    pub fn render(mut self, result: &FormulaLayoutResult) -> FormulaHtml {
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
        self.walk(&result.paint);
        let diagnostics = std::mem::take(&mut self.diagnostics);
        let mut buffer = self.buffer;
        for _ in 0..self.group_depth {
            buffer.push_str("</span>");
        }
        buffer.push_str("</span>");
        FormulaHtml { markup: buffer, diagnostics }
    }

    /// Recursively paint one paint node inside its enclosing group space.
    ///
    /// No ancestor offset is accumulated here: a group's own translate goes on
    /// the group span, and leaves use their node-local positions, so the CSS
    /// nesting composes the canvas position exactly once (see the module-level
    /// coordinate-space contract).
    fn walk(&mut self, node: &PaintNode) {
        match node {
            PaintNode::Group { children, translate_x, translate_y } => {
                let pre = self.buffer.len();
                self.start_group(translate_x.to_f64(), translate_y.to_f64());
                let after_open = self.buffer.len();
                for child in children {
                    self.walk(child);
                }
                if self.buffer.len() == after_open {
                    // The whole subtree painted nothing: drop the group
                    // instead of leaving an empty span in the fragment,
                    // matching the SVG backend's empty-group rule.
                    self.buffer.truncate(pre);
                    self.group_depth -= 1;
                }
                else {
                    self.end_group();
                }
            }
            PaintNode::GlyphRun { chars, family, positions, size, path_data, .. } => {
                let size = size.to_f64();
                for (i, d_opt) in path_data.iter().enumerate() {
                    let Some(d) = d_opt
                    else {
                        // No outline: skip and report, exactly like the SVG
                        // backend's walk over the same entry.
                        self.diagnostics.push(outline_missing(chars.get(i).copied().flatten(), family));
                        continue;
                    };
                    let (px, py) = positions.get(i).copied().unwrap_or((Fixed::ZERO, Fixed::ZERO));
                    self.path(d, px.to_f64(), py.to_f64(), size);
                }
            }
            PaintNode::Rect { x, y, width, height, .. } => {
                self.rect(x.to_f64(), y.to_f64(), width.to_f64(), height.to_f64());
            }
            PaintNode::VectorPath { d, x, y, scale, .. } => {
                self.path(d, x.to_f64(), y.to_f64(), scale.to_f64());
            }
        }
    }

    /// Emit one filled rule rectangle at its group-local top-left `(x, y)`.
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
    /// group-local space, where `(x, y)` is the node's own baseline anchor.
    /// The element's box is the transformed ink bounds (padded), with a
    /// matching `viewBox`, so nothing relies on `overflow: visible` —
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

    /// Open a positioned group span carrying **only** this group's own offset
    /// `(dx, dy)` from the enclosing group origin — never the accumulated
    /// canvas position, which the CSS nesting composes on its own.
    fn start_group(&mut self, dx: f64, dy: f64) {
        use std::fmt::Write;
        let _ = write!(
            self.buffer,
            r#"<span style="position:absolute;left:0;top:0;width:0;height:0;transform:translate({}px,{}px)">"#,
            num(dx),
            num(dy)
        );
        self.group_depth += 1;
    }

    /// Close the innermost group span.
    fn end_group(&mut self) {
        self.buffer.push_str("</span>");
        self.group_depth -= 1;
    }
}

/// Axis-aligned ink bounds of one transformed path, in the group-local canvas
/// pixels the emitted svg fragment is positioned with.
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

/// Structure diagnostic for a glyph the math font ships no outline for.
///
/// The same contract the SVG backend reports: the entry is skipped, the box
/// keeps its measured advance, and no host-font text run replaces the ink.
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
