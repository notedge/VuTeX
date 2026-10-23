pub use rutex_types::{RuTeXError, Result};
pub use rutex_layout::{LayoutNode, LayoutBackend, render_layout_node, Path};

pub struct SvgBackend {
    buffer: String,
    width: f64,
    height: f64,
    group_depth: usize,
}

impl SvgBackend {
    pub fn new(width: f64, height: f64) -> Self {
        Self {
            buffer: String::new(),
            width,
            height,
            group_depth: 0,
        }
    }

    pub fn finish(self) -> String {
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
        final_svg
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
    ) -> Result<()> {
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
        .map_err(|e| RuTeXError::backend_error(e.to_string()))
    }

    fn render_rect(&mut self, x: f64, y: f64, w: f64, h: f64) -> Result<()> {
        use std::fmt::Write;
        write!(
            self.buffer,
            r#"<rect x="{}" y="{}" width="{}" height="{}" fill="currentColor" />"#,
            x, y, w, h
        )
        .map_err(|e| RuTeXError::backend_error(e.to_string()))
    }

    fn render_path(&mut self, d: &str, x: f64, y: f64, scale: f64) -> Result<()> {
        use std::fmt::Write;
        let transform = if scale != 1.0 {
            format!(r#"transform="translate({}, {}) scale({})""#, x, y, scale)
        } else {
            format!(r#"transform="translate({}, {})""#, x, y)
        };
        write!(
            self.buffer,
            r#"<path d="{}" {} fill="currentColor" />"#,
            d, transform
        )
        .map_err(|e| RuTeXError::backend_error(e.to_string()))
    }

    fn start_group(&mut self, transform: Option<&str>) -> Result<()> {
        use std::fmt::Write;
        if let Some(t) = transform {
            write!(self.buffer, r#"<g transform="{}">"#, t)
        } else {
            write!(self.buffer, "<g>")
        }
        .map_err(|e| RuTeXError::backend_error(e.to_string()))?;
        self.group_depth += 1;
        Ok(())
    }

    fn end_group(&mut self) -> Result<()> {
        if self.group_depth > 0 {
            self.buffer.push_str("</g>");
            self.group_depth -= 1;
            Ok(())
        } else {
            Err(RuTeXError::backend_error("No group to end"))
        }
    }
}


