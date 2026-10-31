//! Structured Presentation MathML backend.
//!
//! Renders one [`MathSemantics`] tree — lowered from the parsed math IR — into
//! Presentation MathML.  Every MathML structure (`mfrac`, `msqrt` / `mroot`,
//! `msub` / `msup` / `msubsup`, `mtable`, `mover` / `munder`, `mrow`, fences)
//! comes from the semantic node that produced it, never from a painted glyph
//! run or from a laid-out geometry tree.

use vutex_types::math::{AccentPosition, Alignment, FormulaLayoutResult, LineStyle, MathSemNode, MathSemantics};

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

/// Render the structured semantics carried by a [`FormulaLayoutResult`].
///
/// The structure comes from the semantic tree layout already consumed
/// ([`FormulaLayoutResult::semantics`]), so MathML and the geometry of the same
/// result describe one formula without the source being parsed a second time.
pub fn render_mathml_result(result: &FormulaLayoutResult) -> String {
    let mut backend = MathmlBackend::new();
    backend.render_semantics(&result.semantics);
    backend.finish()
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
        MathSemNode::Table { rows, columnalign } => {
            // An authored column alignment is structure `mtable` can carry, so
            // it is emitted instead of being left to MathML's default.  A table
            // with no authored alignment (a matrix) keeps the bare element.
            if columnalign.is_empty() {
                wrap(out, "mtable", |out| render_table_rows(out, rows));
            }
            else {
                out.push_str(r#"<mtable columnalign=""#);
                out.push_str(&alignment_value(columnalign));
                out.push_str("\">");
                render_table_rows(out, rows);
                out.push_str("</mtable>");
            }
        }
        MathSemNode::Accent { base, accent, position } => {
            let tag = match position {
                AccentPosition::Above => "mover",
                AccentPosition::Below => "munder",
            };
            // A TeX accent is not a bare script: `mover` / `munder` default to
            // `accent="false"`, which lays the second operand out as a
            // full-sized script instead of compressing the accent onto its
            // base.  KaTeX marks the same structure `<mover accent="true">`.
            // Only this variant carries that meaning — [`MathSemNode::StackAbove`]
            // and [`MathSemNode::StackBelow`] are arbitrary stacked expressions
            // (`\overset` / `\underset`) and keep the default.
            wrap_with_attrs(out, tag, r#" accent="true""#, |out| {
                render_node(out, base);
                push_element(out, "mo", accent);
            });
        }
        MathSemNode::StackAbove { base, over } => wrap(out, "mover", |out| {
            render_node(out, base);
            render_node(out, over);
        }),
        MathSemNode::StackBelow { base, under } => wrap(out, "munder", |out| {
            render_node(out, base);
            render_node(out, under);
        }),
        MathSemNode::Delimited { left, right, body } => wrap(out, "mrow", |out| {
            // Every construction site of this node (`\left` / `\right`, `\big`
            // sizing, fenced matrices, `\binom`) authors a delimiter, not an
            // infix operator, so the fences carry `fence="true"` — the same
            // mark KaTeX puts on the `mo` of `\left(\frac{a}{b}\right)`.
            if let Some(left) = left {
                push_element_with_attrs(out, "mo", r#" fence="true""#, left);
            }
            render_node(out, body);
            if let Some(right) = right {
                push_element_with_attrs(out, "mo", r#" fence="true""#, right);
            }
        }),
        MathSemNode::Space { width_em } => {
            out.push_str(&format!(r#"<mspace width="{width_em:.4}em" />"#));
        }
        MathSemNode::Empty => {}
    }
}

/// Emit one `mtr` per table row, one `mtd` per cell.
fn render_table_rows(out: &mut String, rows: &[Vec<MathSemNode>]) {
    for row in rows {
        wrap(out, "mtr", |out| {
            for cell in row {
                wrap(out, "mtd", |out| render_node(out, cell));
            }
        });
    }
}

/// MathML `columnalign` value for a table's per-column alignment.
fn alignment_value(columnalign: &[Alignment]) -> String {
    columnalign
        .iter()
        .map(|alignment| match alignment {
            Alignment::Left => "left",
            Alignment::Center => "center",
            Alignment::Right => "right",
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Emit `<tag>…</tag>` around the content produced by `render`.
fn wrap(out: &mut String, tag: &str, render: impl FnOnce(&mut String)) {
    wrap_with_attrs(out, tag, "", render);
}

/// Emit `<tag attrs…>…</tag>`, where `attrs` is each attribute written with a
/// leading space (e.g. `r#" accent="true""#`).
fn wrap_with_attrs(out: &mut String, tag: &str, attrs: &str, render: impl FnOnce(&mut String)) {
    out.push('<');
    out.push_str(tag);
    out.push_str(attrs);
    out.push('>');
    render(out);
    out.push_str("</");
    out.push_str(tag);
    out.push('>');
}

/// Emit a leaf element with escaped character data.
fn push_element(out: &mut String, tag: &str, text: &str) {
    push_element_with_attrs(out, tag, "", text);
}

/// Emit a leaf element with `attrs` (each written with a leading space) before
/// its escaped character data.
fn push_element_with_attrs(out: &mut String, tag: &str, attrs: &str, text: &str) {
    out.push('<');
    out.push_str(tag);
    out.push_str(attrs);
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

    #[test]
    fn accent_carries_the_accent_attribute() {
        let semantics = MathSemantics {
            nodes: vec![
                MathSemNode::Accent {
                    base: Box::new(MathSemNode::Identifier("x".into())),
                    accent: "ˆ".into(),
                    position: AccentPosition::Above,
                },
                MathSemNode::Accent {
                    base: Box::new(MathSemNode::Identifier("y".into())),
                    accent: "‾".into(),
                    position: AccentPosition::Below,
                },
            ],
        };
        let mut backend = MathmlBackend::new();
        backend.render_semantics(&semantics);
        let mathml = backend.finish();
        assert!(mathml.contains(r#"<mover accent="true"><mi>x</mi><mo>ˆ</mo></mover>"#), "an over-accent is marked: {mathml}");
        assert!(mathml.contains(r#"<munder accent="true"><mi>y</mi><mo>‾</mo></munder>"#), "an under-accent is marked: {mathml}");
    }

    #[test]
    fn stacked_expressions_are_not_accents() {
        // `\overset` / `\underset` stack a full expression, not a compressed
        // accent glyph, so they keep MathML's default `accent="false"`.
        let semantics = MathSemantics {
            nodes: vec![
                MathSemNode::StackAbove {
                    base: Box::new(MathSemNode::Identifier("b".into())),
                    over: Box::new(MathSemNode::Identifier("a".into())),
                },
                MathSemNode::StackBelow {
                    base: Box::new(MathSemNode::Identifier("d".into())),
                    under: Box::new(MathSemNode::Identifier("c".into())),
                },
            ],
        };
        let mut backend = MathmlBackend::new();
        backend.render_semantics(&semantics);
        let mathml = backend.finish();
        assert!(mathml.contains("<mover><mi>b</mi><mi>a</mi></mover>"), "overset stays bare: {mathml}");
        assert!(mathml.contains("<munder><mi>d</mi><mi>c</mi></munder>"), "underset stays bare: {mathml}");
        assert!(!mathml.contains("accent="), "no stack claims accent compression: {mathml}");
    }

    #[test]
    fn authored_delimiters_are_marked_as_fences() {
        let semantics = MathSemantics {
            nodes: vec![
                MathSemNode::Delimited {
                    left: Some("(".into()),
                    right: Some(")".into()),
                    body: Box::new(MathSemNode::Identifier("x".into())),
                },
                // `\left( … \right.` keeps only the authored fence.
                MathSemNode::Delimited {
                    left: Some("[".into()),
                    right: None,
                    body: Box::new(MathSemNode::Identifier("y".into())),
                },
                // An ordinary operator is never a fence.
                MathSemNode::Operator("+".into()),
            ],
        };
        let mut backend = MathmlBackend::new();
        backend.render_semantics(&semantics);
        let mathml = backend.finish();
        assert!(mathml.contains(r#"<mrow><mo fence="true">(</mo><mi>x</mi><mo fence="true">)</mo></mrow>"#), "both fences marked: {mathml}");
        assert!(mathml.contains(r#"<mo fence="true">[</mo><mi>y</mi>"#), "the lone fence is marked: {mathml}");
        assert!(mathml.contains("<mo>+</mo>"), "an infix operator stays bare: {mathml}");
    }
}
