use vutex_layout::math::LayoutBackend;
use vutex_types::math::{MathError, MathResult};

/// Outcome of a render pass: whether any glyphs fell back to host-font <text>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SvgOutcome {
    /// All glyphs were rendered as paths (deterministic, no host-font dependency).
    PathOnly,
    /// Some glyphs were rendered as <text> (non-deterministic, host-font dependent).
    WithText,
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
    /// Grow the bounds by one painted point.
    fn include_point(&mut self, x: f64, y: f64) {
        if !x.is_finite() || !y.is_finite() {
            return;
        }
        self.min_x = self.min_x.min(x);
        self.min_y = self.min_y.min(y);
        self.max_x = self.max_x.max(x);
        self.max_y = self.max_y.max(y);
    }

    /// Grow the bounds by another bounds rectangle.
    fn include_bounds(&mut self, other: &Bounds) {
        self.include_point(other.min_x, other.min_y);
        self.include_point(other.max_x, other.max_y);
    }
}

/// SVG backend for math layout trees.
///
/// Glyph outlines arrive in em units with y growing upward from the baseline
/// (the OpenType convention). Every path is emitted under
/// `translate(x, y) scale(size, -size)`, which maps the outline onto the
/// y-down canvas with its origin on the baseline. Painted ink is tracked so
/// [`SvgBackend::finish`] can widen the viewport instead of clipping glyphs.
#[derive(Debug)]
pub struct SvgBackend {
    buffer: String,
    width: f64,
    height: f64,
    group_depth: usize,
    /// When true, `render_text` returns an error instead of emitting <text>.
    /// Callers must ensure all glyphs have pre-computed path data for deterministic output.
    strict: bool,
    /// Whether any <text> nodes were emitted during this render.
    text_used: bool,
    /// Warnings collected during rendering (e.g. skipped <text> in strict mode).
    warnings: Vec<String>,
    /// Translation accumulated from enclosing groups, in canvas pixels.
    translate: (f64, f64),
    /// Saved translations for each open group.
    translate_stack: Vec<(f64, f64)>,
    /// Bounds of everything painted so far, in canvas pixels.
    ink: Option<Bounds>,
}

impl SvgBackend {
    /// Create a backend for a canvas of `width` × `height` pixels (non-strict, host-font allowed).
    pub fn new(width: f64, height: f64) -> Self {
        Self {
            buffer: String::new(),
            width,
            height,
            group_depth: 0,
            strict: false,
            text_used: false,
            warnings: Vec::new(),
            translate: (0.0, 0.0),
            translate_stack: Vec::new(),
            ink: None,
        }
    }

    /// Create a backend that forbids host-font <text>.
    ///
    /// If any glyph lacks pre-computed path data, `render_text` returns
    /// [`MathError::backend_error`] so callers can surface the failure.
    pub fn new_deterministic(width: f64, height: f64) -> Self {
        Self { strict: true, ..Self::new(width, height) }
    }

    /// Whether this backend is in deterministic (path-only) mode.
    pub fn is_strict(&self) -> bool {
        self.strict
    }

    /// Warnings collected during rendering.
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// Close the document and return the complete SVG string.
    pub fn finish(self) -> String {
        self.finish_with_outcome().0
    }

    /// Close the document and return both the SVG string and a [`SvgOutcome`]
    /// indicating whether any host-font <text> nodes were emitted.
    ///
    /// The viewport is the union of the requested layout box and the ink that
    /// was actually painted, expanded by a small padding, so scaled glyph
    /// outlines are never clipped by the canvas edges.
    pub fn finish_with_outcome(mut self) -> (String, SvgOutcome) {
        let outcome = if self.text_used { SvgOutcome::WithText } else { SvgOutcome::PathOnly };
        let mut bounds = Bounds { min_x: 0.0, min_y: 0.0, max_x: self.width, max_y: self.height };
        if let Some(ink) = self.ink {
            bounds.include_bounds(&ink);
        }
        let span = (bounds.max_x - bounds.min_x).max(bounds.max_y - bounds.min_y).max(1.0);
        let pad = (span * 0.04).max(0.5);
        let min_x = bounds.min_x - pad;
        let min_y = bounds.min_y - pad;
        let view_w = (bounds.max_x - bounds.min_x) + 2.0 * pad;
        let view_h = (bounds.max_y - bounds.min_y) + 2.0 * pad;
        let mut final_svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}" viewBox="{} {} {} {}">"#,
            num(view_w),
            num(view_h),
            num(min_x),
            num(min_y),
            num(view_w),
            num(view_h)
        );
        final_svg.push_str(&self.buffer);
        // Close any remaining groups just in case
        for _ in 0..self.group_depth {
            final_svg.push_str("</g>");
        }
        final_svg.push_str("</svg>");
        // Suppress dead-field warning if we ever want to reuse self after finish.
        let _ = std::mem::take(&mut self.warnings);
        (final_svg, outcome)
    }
}

impl LayoutBackend for SvgBackend {
    fn render_text(&mut self, text: &str, x: f64, y: f64, font_size: f64, font_family: Option<&str>) -> MathResult<()> {
        // Whitespace advances but paints nothing. Emitting a `<text>` run for it
        // would pull in a host font, which deterministic mode forbids, and the
        // advance is already accounted for by the parent box.
        if text.chars().all(char::is_whitespace) {
            return Ok(());
        }
        if self.strict {
            let msg = format!(
                "deterministic SVG requires pre-computed glyph paths; \
                 <text> fallback triggered for text={:?} at ({}, {})",
                text, x, y
            );
            self.warnings.push(msg.clone());
            return Err(MathError::backend_error(msg));
        }
        self.text_used = true;
        // Rough advance estimate: the backend only needs it to keep the
        // viewport wide enough for the host-font fallback.
        let (tx, ty) = self.translate;
        let estimate = font_size * 0.6 * text.chars().count() as f64;
        let box_bounds =
            Bounds { min_x: tx + x, min_y: ty + y - font_size, max_x: tx + x + estimate, max_y: ty + y + font_size * 0.25 };
        self.ink.get_or_insert(box_bounds).include_bounds(&box_bounds);
        use std::fmt::Write;
        let font_family_attr = if let Some(family) = font_family {
            if family == "default" { String::new() } else { format!(r#" font-family="{}""#, family) }
        }
        else {
            String::new()
        };
        write!(
            self.buffer,
            r#"<text x="{}" y="{}" font-size="{}"{} fill="currentColor">{}</text>"#,
            x, y, font_size, font_family_attr, text
        )
        .map_err(|e| MathError::backend_error(e.to_string()))
    }

    fn render_rect(&mut self, x: f64, y: f64, w: f64, h: f64) -> MathResult<()> {
        let (tx, ty) = self.translate;
        let rect = Bounds { min_x: tx + x, min_y: ty + y, max_x: tx + x + w, max_y: ty + y + h };
        self.ink.get_or_insert(rect).include_bounds(&rect);
        use std::fmt::Write;
        write!(self.buffer, r#"<rect x="{}" y="{}" width="{}" height="{}" fill="currentColor" />"#, x, y, w, h)
            .map_err(|e| MathError::backend_error(e.to_string()))
    }

    fn render_path(&mut self, d: &str, x: f64, y: f64, scale: f64) -> MathResult<()> {
        let (tx, ty) = self.translate;
        let left = tx + x;
        let top = ty + y;
        let mut ink = self.ink;
        visit_path_points(d, |px, py| {
            let wx = left + scale * px;
            let wy = top - scale * py;
            let bounds = ink.get_or_insert(Bounds { min_x: wx, min_y: wy, max_x: wx, max_y: wy });
            bounds.include_point(wx, wy);
        });
        self.ink = ink;
        use std::fmt::Write;
        write!(
            self.buffer,
            r#"<path d="{}" transform="translate({}, {}) scale({}, {})" fill="currentColor" />"#,
            d,
            num(x),
            num(y),
            num(scale),
            num(-scale)
        )
        .map_err(|e| MathError::backend_error(e.to_string()))
    }

    fn start_group(&mut self, transform: Option<&str>) -> MathResult<()> {
        use std::fmt::Write;
        self.translate_stack.push(self.translate);
        if let Some(offset) = transform.and_then(parse_translate) {
            self.translate = (self.translate.0 + offset.0, self.translate.1 + offset.1);
        }
        if let Some(t) = transform { write!(self.buffer, r#"<g transform="{}">"#, t) } else { write!(self.buffer, "<g>") }
            .map_err(|e| MathError::backend_error(e.to_string()))?;
        self.group_depth += 1;
        Ok(())
    }

    fn end_group(&mut self) -> MathResult<()> {
        if self.group_depth > 0 {
            self.buffer.push_str("</g>");
            self.group_depth -= 1;
            self.translate = self.translate_stack.pop().unwrap_or((0.0, 0.0));
            Ok(())
        }
        else {
            Err(MathError::backend_error("No group to end"))
        }
    }
}

/// Format a canvas coordinate for the SVG output.
fn num(value: f64) -> String {
    format!("{value:.4}")
}

/// Parse the `translate(x, y)` transform that layout boxes emit.
///
/// Any other transform string leaves the accumulated translation untouched.
fn parse_translate(transform: &str) -> Option<(f64, f64)> {
    let rest = transform.strip_prefix("translate(")?.strip_suffix(')')?;
    let (x, y) = rest.split_once(',')?;
    Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
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
