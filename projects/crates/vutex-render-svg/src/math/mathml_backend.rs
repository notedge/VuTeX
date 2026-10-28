//! Structured Presentation MathML backend.
//!
//! Renders one [`MathSemantics`] tree — lowered from the parsed math IR — into
//! Presentation MathML.  Every MathML structure (`mfrac`, `msqrt` / `mroot`,
//! `msub` / `msup` / `msubsup`, `mtable`, `mover` / `munder`, `mrow`, fences)
//! comes from the semantic node that produced it, never from a painted glyph
//! run or from a laid-out geometry tree.

use vutex_types::math::{AccentPosition, LineStyle, MathSemNode, MathSemantics};

/// Presentation MathML backend for structured math semantics.
#[derive(Debug, Default)]
pub struct MathmlBackend {
    buffer: String,
}

impl MathmlBackend {
    /// Create an empty MathML backend.
    pub fn new() -> Self {
        Self { buffer: String::new() }
    }

    /// Render one semantic tree into this backend, replacing any previous
    /// content.
    pub fn render_semantics(&mut self, semantics: &MathSemantics) {
        let mut buffer = String::new();
        render_nodes(&mut buffer, &semantics.nodes);
        self.buffer = buffer;
    }

    /// Close the document and return the complete MathML string.
    pub fn finish(self) -> String {
        format!(r#"<math xmlns="http://www.w3.org/1998/Math/MathML" display="block"><mrow>{}</mrow></math>"#, self.buffer)
    }
}

/// Emit every sibling of a row in source order.
fn render_nodes(out: &mut String, nodes: &[MathSemNode]) {
    for node in nodes {
        render_node(out, node);
    }
}

/// Emit one semantic node as its Presentation MathML element.
fn render_node(out: &mut String, node: &MathSemNode) {
    match node {
        MathSemNode::Row(children) => wrap(out, "mrow", |out| render_nodes(out, children)),
        MathSemNode::Identifier(text) => push_element(out, "mi", text),
        MathSemNode::Number(text) => push_element(out, "mn", text),
        MathSemNode::Operator(text) => push_element(out, "mo", text),
        MathSemNode::Text(text) => push_element(out, "mtext", text),
        MathSemNode::Fraction { numerator, denominator, line } => {
            if *line == LineStyle::None {
                // A line-less fraction is a binomial: keep that structure.
                out.push_str(r#"<mfrac linethickness="0">"#);
                render_node(out, numerator);
                render_node(out, denominator);
                out.push_str("</mfrac>");
            }
            else {
                wrap(out, "mfrac", |out| {
                    render_node(out, numerator);
                    render_node(out, denominator);
                });
            }
        }
        MathSemNode::Radical { radicand, degree } => match degree {
            Some(degree) => wrap(out, "mroot", |out| {
                render_node(out, radicand);
                render_node(out, degree);
            }),
            None => wrap(out, "msqrt", |out| render_node(out, radicand)),
        },
        MathSemNode::Sub { base, sub } => wrap(out, "msub", |out| {
            render_node(out, base);
            render_node(out, sub);
        }),
        MathSemNode::Sup { base, sup } => wrap(out, "msup", |out| {
            render_node(out, base);
            render_node(out, sup);
        }),
        MathSemNode::SubSup { base, sub, sup } => wrap(out, "msubsup", |out| {
            render_node(out, base);
            render_node(out, sub);
            render_node(out, sup);
        }),
        MathSemNode::Table { rows } => wrap(out, "mtable", |out| {
            for row in rows {
                wrap(out, "mtr", |out| {
                    for cell in row {
                        wrap(out, "mtd", |out| render_node(out, cell));
                    }
                });
            }
        }),
        MathSemNode::Accent { base, accent, position } => {
            let tag = match position {
                AccentPosition::Above => "mover",
                AccentPosition::Below => "munder",
            };
            wrap(out, tag, |out| {
                render_node(out, base);
                push_element(out, "mo", accent);
            });
        }
        MathSemNode::Delimited { left, right, body } => wrap(out, "mrow", |out| {
            if let Some(left) = left {
                push_element(out, "mo", left);
            }
            render_node(out, body);
            if let Some(right) = right {
                push_element(out, "mo", right);
            }
        }),
        MathSemNode::Space { width_em } => {
            out.push_str(&format!(r#"<mspace width="{width_em:.4}em" />"#));
        }
        MathSemNode::Empty => {}
    }
}

/// Emit `<tag>…</tag>` around the content produced by `render`.
fn wrap(out: &mut String, tag: &str, render: impl FnOnce(&mut String)) {
    out.push('<');
    out.push_str(tag);
    out.push('>');
    render(out);
    out.push_str("</");
    out.push_str(tag);
    out.push('>');
}

/// Emit a leaf element with escaped character data.
fn push_element(out: &mut String, tag: &str, text: &str) {
    out.push('<');
    out.push_str(tag);
    out.push('>');
    out.push_str(&escape_xml(text));
    out.push_str("</");
    out.push_str(tag);
    out.push('>');
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_semantics_still_emits_a_document() {
        let mut backend = MathmlBackend::new();
        backend.render_semantics(&MathSemantics::default());
        let mathml = backend.finish();
        assert!(mathml.starts_with("<math"));
        assert!(mathml.ends_with("</math>"));
        assert!(mathml.contains("<mrow></mrow>"));
    }

    #[test]
    fn sparse_atoms_emit_their_own_elements() {
        let semantics = MathSemantics {
            nodes: vec![
                MathSemNode::Identifier("x".into()),
                MathSemNode::Number("7".into()),
                MathSemNode::Operator("+".into()),
                MathSemNode::Text("sin".into()),
                MathSemNode::Space { width_em: 1.0 },
            ],
        };
        let mut backend = MathmlBackend::new();
        backend.render_semantics(&semantics);
        let mathml = backend.finish();
        assert!(mathml.contains("<mi>x</mi>"));
        assert!(mathml.contains("<mn>7</mn>"));
        assert!(mathml.contains("<mo>+</mo>"));
        assert!(mathml.contains("<mtext>sin</mtext>"));
        assert!(mathml.contains(r#"<mspace width="1.0000em" />"#));
    }
}
