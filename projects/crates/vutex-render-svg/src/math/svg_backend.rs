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

/// SVG backend for math layout trees.
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
        }
    }

    /// Create a backend that forbids host-font <text>.
    ///
    /// If any glyph lacks pre-computed path data, `render_text` returns
    /// [`MathError::backend_error`] so callers can surface the failure.
    pub fn new_deterministic(width: f64, height: f64) -> Self {
        Self {
            buffer: String::new(),
            width,
            height,
            group_depth: 0,
            strict: true,
            text_used: false,
            warnings: Vec::new(),
        }
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
    pub fn finish_with_outcome(mut self) -> (String, SvgOutcome) {
        let outcome = if self.text_used { SvgOutcome::WithText } else { SvgOutcome::PathOnly };
        let mut final_svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}" viewBox="0 0 {} {}">"#,
            self.width, self.height, self.width, self.height
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
    fn render_text(
        &mut self,
        text: &str,
        x: f64,
        y: f64,
        font_size: f64,
        font_family: Option<&str>,
    ) -> MathResult<()> {
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
        use std::fmt::Write;
        let font_family_attr = if let Some(family) = font_family {
            if family == "default" {
                String::new()
            } else {
                format!(r#" font-family="{}""#, family)
            }
        } else {
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
        use std::fmt::Write;
        write!(
            self.buffer,
            r#"<rect x="{}" y="{}" width="{}" height="{}" fill="currentColor" />"#,
            x, y, w, h
        )
        .map_err(|e| MathError::backend_error(e.to_string()))
    }

    fn render_path(&mut self, d: &str, x: f64, y: f64, scale: f64) -> MathResult<()> {
        use std::fmt::Write;
        let transform = if scale != 1.0 {
            format!(r#"transform="translate({}, {}) scale({})""#, x, y, scale)
        } else {
            format!(r#"transform="translate({}, {})""#, x, y)
        };
        write!(self.buffer, r#"<path d="{}" {} fill="currentColor" />"#, d, transform)
            .map_err(|e| MathError::backend_error(e.to_string()))
    }

    fn start_group(&mut self, transform: Option<&str>) -> MathResult<()> {
        use std::fmt::Write;
        if let Some(t) = transform {
            write!(self.buffer, r#"<g transform="{}">"#, t)
        } else {
            write!(self.buffer, "<g>")
        }
        .map_err(|e| MathError::backend_error(e.to_string()))?;
        self.group_depth += 1;
        Ok(())
    }

    fn end_group(&mut self) -> MathResult<()> {
        if self.group_depth > 0 {
            self.buffer.push_str("</g>");
            self.group_depth -= 1;
            Ok(())
        } else {
            Err(MathError::backend_error("No group to end"))
        }
    }
}
