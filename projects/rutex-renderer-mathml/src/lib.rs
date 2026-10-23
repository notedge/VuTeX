pub use rutex_types::{RuTeXError, Result};
pub use rutex_layout::{LayoutNode, LayoutBackend, render_layout_node, Path};

pub struct MathmlBackend {
    buffer: String,
    tag_stack: Vec<&'static str>,
}

impl MathmlBackend {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            tag_stack: Vec::new(),
        }
    }

    pub fn finish(mut self) -> String {
        let mut final_mml = String::from(r#"<math xmlns="http://www.w3.org/1998/Math/MathML" display="block">"#);
        
        // Add a top-level mrow to hold everything
        final_mml.push_str("<mrow>");
        final_mml.push_str(&self.buffer);
        
        // Close any remaining tags
        while let Some(tag) = self.tag_stack.pop() {
            final_mml.push_str("</");
            final_mml.push_str(tag);
            final_mml.push_str(">");
        }
        
        final_mml.push_str("</mrow>");
        final_mml.push_str("</math>");
        final_mml
    }
}

impl LayoutBackend for MathmlBackend {
    fn render_text(
        &mut self,
        text: &str,
        x: f64,
        y: f64,
        font_size: f64,
        font_family: Option<&str>,
    ) -> Result<()> {
        use std::fmt::Write;
        
        // Use <mpadded> for absolute positioning relative to the group
        write!(
            self.buffer,
            r#"<mpadded voffset="{}px" loffset="{}px">"#,
            y, x
        ).map_err(|e| RuTeXError::backend_error(e.to_string()))?;

        // Determine the MathML tag based on character type
        let tag = if text.chars().count() == 1 {
            let c = text.chars().next().unwrap();
            if c.is_numeric() {
                "mn"
            } else if c.is_alphabetic() {
                "mi"
            } else {
                "mo"
            }
        } else {
            "mtext"
        };

        let mut style = format!("font-size: {}px;", font_size);
        if let Some(family) = font_family {
            write!(style, " font-family: {};", family).unwrap();
        }

        write!(
            self.buffer,
            r#"<{tag} style="{}">{}</{tag}>"#,
            style, escape_xml(text)
        ).map_err(|e| RuTeXError::backend_error(e.to_string()))?;
        
        self.buffer.push_str("</mpadded>");
        Ok(())
    }

    fn render_rect(&mut self, x: f64, y: f64, w: f64, h: f64) -> Result<()> {
        use std::fmt::Write;
        // Use <mspace> with a background color for rules
        write!(
            self.buffer,
            r#"<mpadded voffset="{}px" loffset="{}px"><mspace width="{}px" height="{}px" style="background: currentColor;" /></mpadded>"#,
            y, x, w, h
        )
        .map_err(|e| RuTeXError::backend_error(e.to_string()))
    }

    fn render_path(&mut self, d: &str, x: f64, y: f64, scale: f64) -> Result<()> {
        use std::fmt::Write;
        let transform = if scale != 1.0 {
            format!(r#" transform="scale({})""#, scale)
        } else {
            String::new()
        };
        // Embed a small SVG inside MathML
        write!(
            self.buffer,
            r#"<mpadded voffset="{}px" loffset="{}px"><mtext><svg xmlns="http://www.w3.org/2000/svg" width="100%" height="100%" style="overflow: visible;"><path d="{}"{} fill="currentColor" /></svg></mtext></mpadded>"#,
            y, d, transform
        )
        .map_err(|e| RuTeXError::backend_error(e.to_string()))
    }

    fn start_group(&mut self, transform: Option<&str>) -> Result<()> {
        use std::fmt::Write;
        
        if let Some(t) = transform {
            if t.starts_with("translate(") {
                // Parse translate(x, y)
                let content = &t[10..t.len() - 1];
                let parts: Vec<&str> = content.split(',').map(|s| s.trim()).collect();
                if parts.len() == 2 {
                    let tx: f64 = parts[0].parse().unwrap_or(0.0);
                    let ty: f64 = parts[1].parse().unwrap_or(0.0);
                    
                    write!(self.buffer, r#"<mpadded voffset="{}px" loffset="{}px">"#, ty, tx)
                        .map_err(|e| RuTeXError::backend_error(e.to_string()))?;
                    self.tag_stack.push("mpadded");
                    return Ok(());
                }
            }
        }
        
        self.buffer.push_str("<mrow>");
        self.tag_stack.push("mrow");
        Ok(())
    }

    fn end_group(&mut self) -> Result<()> {
        if let Some(tag) = self.tag_stack.pop() {
            use std::fmt::Write;
            write!(self.buffer, "</{}>", tag)
                .map_err(|e| RuTeXError::backend_error(e.to_string()))
        } else {
            Err(RuTeXError::backend_error("No group to end"))
        }
    }
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}




