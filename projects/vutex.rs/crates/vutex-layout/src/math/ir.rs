//! Lower `notedown-ir` math semantics into VuTeX's layout nodes.
//!
//! The document IR owns the mathematical structure; this module only maps
//! that structure onto the node types the layout engine understands. It never
//! parses source text and never invents syntax: an unsupported expression
//! becomes visible text plus a diagnostic.

use notedown_ir::{
    MathClass, MathColumnAlignment, MathDelimiter, MathEnvironment, MathEnvironmentKind, MathExpression, MathFormula, MathRowNumbering, MathSpace,
    MathStyle, MathStyleKind,
};
use vutex_types::{
    Diagnostic, Severity,
    math::{
        Alignment, EnvironmentKind, EnvironmentRow, Fixed, FontStyle, GlyphKey, LimitsPolicy, LineStyle, MathEnvironment as LayoutEnvironment,
        MathSemanticTree, MathStyle as LayoutStyle, Numbering, SemanticNode, SymbolRole,
    },
};

/// Lower one formula into the semantic tree the layout engine consumes.
pub fn semantic_tree(formula: &MathFormula) -> (MathSemanticTree, Vec<Diagnostic>) {
    let mut diagnostics = Vec::new();
    let root = semantic_node(&formula.expression, None, &mut diagnostics);
    (MathSemanticTree { root }, diagnostics)
}

/// Lower one expression, optionally inside a face scope.
fn semantic_node(expression: &MathExpression, face: Option<FontStyle>, diagnostics: &mut Vec<Diagnostic>) -> SemanticNode {
    match expression {
        MathExpression::Sequence(items) => {
            let content: Vec<SemanticNode> = items.iter().map(|item| semantic_node(item, face, diagnostics)).collect();
            for pair in items.windows(2) {
                let (MathExpression::LimitsPolicy { limits }, MathExpression::Script { base, .. }) = (&pair[0], &pair[1])
                else {
                    continue;
                };
                if !is_operator_base(base) {
                    let command = if *limits { "\\limits" } else { "\\nolimits" };
                    diagnostics.push(Diagnostic {
                        code: "vutex.layout.math.limits_misuse".into(),
                        severity: Severity::Warning,
                        message: format!(
                            "`{command}` applies only to a large operator such as `\\sum`, so this atom keeps its scripts beside the base"
                        ),
                    });
                }
            }
            collapse(content)
        }
        MathExpression::Atom(atom) => atom_node(atom.text.as_str(), atom.class, face, diagnostics),
        MathExpression::Number { text } => {
            if text.chars().count() > 1 && face.is_none() {
                SemanticNode::Number { text: text.clone() }
            }
            else {
                let style = face.unwrap_or(FontStyle::Normal);
                SemanticNode::Symbol { glyph_key: GlyphKey::from_char(text.chars().next().unwrap_or('0'), None, style), role: SymbolRole::Ordinary }
            }
        }
        MathExpression::Group { body } => semantic_node(body, face, diagnostics),
        MathExpression::Script { base, superscript, subscript } => {
            let base = semantic_node(base, face, diagnostics);
            match (subscript, superscript) {
                (Some(sub), Some(sup)) => SemanticNode::SubSuperscript {
                    base: Box::new(base),
                    sub: Box::new(semantic_node(sub, face, diagnostics)),
                    sup: Box::new(semantic_node(sup, face, diagnostics)),
                },
                (Some(sub), None) => {
                    SemanticNode::Subscript { base: Box::new(base), sub: Box::new(semantic_node(sub, face, diagnostics)) }
                }
                (None, Some(sup)) => {
                    SemanticNode::Superscript { base: Box::new(base), sup: Box::new(semantic_node(sup, face, diagnostics)) }
                }
                (None, None) => base,
            }
        }
        MathExpression::Fraction { numerator, denominator, line } => SemanticNode::Fraction {
            num: Box::new(semantic_node(numerator, face, diagnostics)),
            den: Box::new(semantic_node(denominator, face, diagnostics)),
            line: match line {
                notedown_ir::MathFractionLine::Solid => LineStyle::Solid,
                notedown_ir::MathFractionLine::None => LineStyle::None,
            },
        },
        MathExpression::Radical { index, radicand } => SemanticNode::Radical {
            degree: index.as_ref().map(|index| Box::new(semantic_node(index, face, diagnostics))),
            radicand: Box::new(semantic_node(radicand, face, diagnostics)),
        },
        MathExpression::Delimited { open, close, body, middles } => {
            let left = delimiter_key(open);
            let right = delimiter_key(close);
            let content = semantic_node(body, face, diagnostics);
            for middle in middles {
                let _ = semantic_node(middle, face, diagnostics);
                diagnostics.push(unsupported("middle-delimiter", "a `\\middle` delimiter has no VuTeX layout node"));
            }
            SemanticNode::Delimited { left, right, content: Box::new(content) }
        }
        MathExpression::Styled { style, body } => {
            let mapped = face_style(*style);
            semantic_node(body, Some(mapped), diagnostics)
        }
        MathExpression::Accent { mark, wide, base } => {
            let _ = wide;
            SemanticNode::Accent {
                base: Box::new(semantic_node(base, face, diagnostics)),
                accent: GlyphKey::from_char(*mark, None, FontStyle::Normal),
            }
        }
        MathExpression::StackAbove { over, base } => SemanticNode::OverSet {
            top: Box::new(semantic_node(over, face, diagnostics)),
            base: Box::new(semantic_node(base, face, diagnostics)),
        },
        MathExpression::StackBelow { under, base } => SemanticNode::UnderSet {
            bot: Box::new(semantic_node(under, face, diagnostics)),
            base: Box::new(semantic_node(base, face, diagnostics)),
        },
        MathExpression::Relation(inner) => SemanticNode::Relation(Box::new(semantic_node(inner, face, diagnostics))),
        MathExpression::Space { kind } => SemanticNode::Space { width: space_width(*kind) },
        MathExpression::StyleSwitch { style } => SemanticNode::SetMathStyle(match style {
            MathStyleKind::Display => LayoutStyle::Display,
            MathStyleKind::Text => LayoutStyle::Text,
            MathStyleKind::Script => LayoutStyle::Script,
            MathStyleKind::ScriptScript => LayoutStyle::ScriptScript,
        }),
        MathExpression::LimitsPolicy { limits } => {
            SemanticNode::LimitsSpec(if *limits { LimitsPolicy::Limits } else { LimitsPolicy::NoLimits })
        }
        MathExpression::Environment(environment) => environment_node(environment, face, diagnostics),
        MathExpression::Text { text } => SemanticNode::Text {
            text: text.clone(),
            font_style: match face {
                Some(FontStyle::Italic) => FontStyle::Italic,
                _ => FontStyle::Normal,
            },
        },
        MathExpression::Unsupported { kind, source } => {
            diagnostics.push(unsupported(kind, source));
            SemanticNode::Text { text: source.clone(), font_style: FontStyle::Normal }
        }
    }
}

fn atom_node(text: &str, class: MathClass, face: Option<FontStyle>, diagnostics: &mut Vec<Diagnostic>) -> SemanticNode {
    let mut chars = text.chars();
    let Some(character) = chars.next() else {
        diagnostics.push(unsupported("empty-atom", text));
        return SemanticNode::Sequence(Vec::new());
    };
    if chars.next().is_some() {
        return SemanticNode::Text { text: text.to_string(), font_style: FontStyle::Normal };
    }
    let style = face.unwrap_or_else(|| default_face(class));
    SemanticNode::Symbol { glyph_key: GlyphKey::from_char(character, None, style), role: role_for(class) }
}

fn default_face(class: MathClass) -> FontStyle {
    match class {
        MathClass::Opening | MathClass::Closing | MathClass::Punctuation | MathClass::Number => FontStyle::Normal,
        _ => FontStyle::Math,
    }
}

fn role_for(class: MathClass) -> SymbolRole {
    match class {
        MathClass::Binary => SymbolRole::Binary,
        MathClass::Relation => SymbolRole::Relation,
        MathClass::Opening => SymbolRole::Opening,
        MathClass::Closing => SymbolRole::Closing,
        MathClass::Punctuation => SymbolRole::Punctuation,
        MathClass::Operator => SymbolRole::Operator,
        MathClass::LargeOperator => SymbolRole::LargeOperator,
        MathClass::Inner => SymbolRole::Inner,
        MathClass::Identifier | MathClass::Number | MathClass::Ordinary | MathClass::Space => SymbolRole::Ordinary,
    }
}

fn face_style(style: MathStyle) -> FontStyle {
    match style {
        MathStyle::Upright => FontStyle::Normal,
        MathStyle::Italic => FontStyle::Italic,
        MathStyle::Bold => FontStyle::Bold,
        MathStyle::DoubleStruck => FontStyle::DoubleStruck,
        MathStyle::Script => FontStyle::Calligraphic,
        MathStyle::Fraktur => FontStyle::Fraktur,
        MathStyle::SansSerif => FontStyle::SansSerif,
        MathStyle::Monospace => FontStyle::Monospace,
    }
}

fn space_width(kind: MathSpace) -> Fixed {
    let em = match kind {
        MathSpace::Thin => 3.0 / 18.0,
        MathSpace::Medium => 4.0 / 18.0,
        MathSpace::Thick => 5.0 / 18.0,
        MathSpace::Interword => 6.0 / 18.0,
        MathSpace::Quad => 1.0,
        MathSpace::DoubleQuad => 2.0,
        MathSpace::NegativeThin => 0.0,
    };
    Fixed::from_f64(em)
}

fn delimiter_key(delimiter: &Option<MathDelimiter>) -> Option<GlyphKey> {
    delimiter.as_ref().and_then(|delimiter| delimiter.character).map(|character| GlyphKey::from_char(character, None, FontStyle::Normal))
}

fn environment_node(environment: &MathEnvironment, face: Option<FontStyle>, diagnostics: &mut Vec<Diagnostic>) -> SemanticNode {
    let rows: Vec<Vec<SemanticNode>> = environment
        .rows
        .iter()
        .map(|row| row.cells.iter().map(|cell| semantic_node(cell, face, diagnostics)).collect())
        .collect();
    match environment.kind {
        MathEnvironmentKind::Matrix | MathEnvironmentKind::Cases => {
            let columns = rows.iter().map(|row| row.len()).max().unwrap_or(0);
            let column_alignment = match environment.kind {
                MathEnvironmentKind::Cases => vec![Alignment::Left; columns],
                _ => vec![Alignment::Center; columns],
            };
            let row_spacing = if environment.name == "smallmatrix" { 0.2 } else { 0.35 };
            let matrix = SemanticNode::Matrix {
                rows,
                row_spacing: Fixed::from_f64(row_spacing),
                col_spacing: Fixed::from_f64(0.8),
                display_style: false,
                column_alignment,
            };
            let (left, right) = matrix_delimiters(&environment.name);
            match (left, right) {
                (None, None) => matrix,
                _ => SemanticNode::Delimited {
                    left: left.map(|character| GlyphKey::from_char(character, None, FontStyle::Normal)),
                    right: right.map(|character| GlyphKey::from_char(character, None, FontStyle::Normal)),
                    content: Box::new(matrix),
                },
            }
        }
        MathEnvironmentKind::Aligned
        | MathEnvironmentKind::AlignedAt
        | MathEnvironmentKind::Split
        | MathEnvironmentKind::Align
        | MathEnvironmentKind::Gather
        | MathEnvironmentKind::Gathered
        | MathEnvironmentKind::Multline
        | MathEnvironmentKind::Equation => {
            let kind = layout_environment_kind(environment);
            if let EnvironmentKind::AlignedAt { pairs } = kind {
                let declared = pairs as usize * 2;
                let typed = rows.iter().map(Vec::len).max().unwrap_or(0);
                if declared != typed {
                    diagnostics.push(Diagnostic {
                        code: "vutex.layout.math.alignedat_columns".into(),
                        severity: Severity::Warning,
                        message: format!(
                            "`\\begin{{alignedat}}` declares {pairs} column pairs ({declared} columns) but its body has {typed} columns, so the grid keeps {} columns",
                            declared.max(typed)
                        ),
                    });
                }
            }
            let rows: Vec<EnvironmentRow> = environment
                .rows
                .iter()
                .map(|row| EnvironmentRow { cells: row.cells.iter().map(|cell| semantic_node(cell, face, diagnostics)).collect(), numbering: numbering(&row.numbering) })
                .collect();
            let col_spacing = match kind.anchors() {
                vutex_types::math::AlignmentAnchors::ColumnPairs | vutex_types::math::AlignmentAnchors::SingleEquation => 0.0,
                _ => 0.8,
            };
            SemanticNode::Environment(LayoutEnvironment {
                kind,
                starred: environment.starred,
                rows,
                row_spacing: Fixed::from_f64(0.35),
                col_spacing: Fixed::from_f64(col_spacing),
            })
        }
        MathEnvironmentKind::AlignAt | MathEnvironmentKind::FlAlign | MathEnvironmentKind::EqnArray | MathEnvironmentKind::Other(_) => {
            diagnostics.push(unsupported("environment", &environment.name));
            SemanticNode::Text { text: environment.name.clone(), font_style: FontStyle::Normal }
        }
    }
}

fn layout_environment_kind(environment: &MathEnvironment) -> EnvironmentKind {
    match environment.kind {
        MathEnvironmentKind::Aligned => EnvironmentKind::Aligned,
        MathEnvironmentKind::AlignedAt => {
            let pairs = environment.column_pairs.unwrap_or_else(|| {
                let columns = environment.rows.iter().map(|row| row.cells.len()).max().unwrap_or(0);
                (columns.div_ceil(2)) as u32
            });
            EnvironmentKind::AlignedAt { pairs }
        }
        MathEnvironmentKind::Split => EnvironmentKind::Split,
        MathEnvironmentKind::Align => EnvironmentKind::Align,
        MathEnvironmentKind::Gather => EnvironmentKind::Gather,
        MathEnvironmentKind::Gathered => EnvironmentKind::Gathered,
        MathEnvironmentKind::Multline => EnvironmentKind::Multline,
        _ => EnvironmentKind::Equation,
    }
}

fn numbering(numbering: &MathRowNumbering) -> Numbering {
    match numbering {
        MathRowNumbering::Tagged(label) => Numbering::Tag(label.clone()),
        MathRowNumbering::Suppressed => Numbering::Suppressed,
        MathRowNumbering::None | MathRowNumbering::Numbered => Numbering::Default,
    }
}

fn matrix_delimiters(name: &str) -> (Option<char>, Option<char>) {
    match name {
        "pmatrix" => (Some('('), Some(')')),
        "bmatrix" => (Some('['), Some(']')),
        "Bmatrix" => (Some('{'), Some('}')),
        "vmatrix" => (Some('|'), Some('|')),
        "Vmatrix" => (Some('‖'), Some('‖')),
        "cases" => (Some('{'), None),
        _ => (None, None),
    }
}

fn is_operator_base(expression: &MathExpression) -> bool {
    match expression {
        MathExpression::Atom(atom) => atom.class == MathClass::LargeOperator,
        MathExpression::Text { .. } => true,
        MathExpression::Group { body } => is_operator_base(body),
        _ => false,
    }
}

fn collapse(mut items: Vec<SemanticNode>) -> SemanticNode {
    match items.len() {
        0 => SemanticNode::Sequence(Vec::new()),
        1 => items.remove(0),
        _ => SemanticNode::Sequence(items),
    }
}

fn unsupported(kind: &str, source: &str) -> Diagnostic {
    Diagnostic {
        code: "vutex.math.unsupported_expression".into(),
        severity: Severity::Warning,
        message: format!("math {kind} has no VuTeX layout mapping and stays plain text: `{source}`"),
    }
}

/// Whether a formula carries the TeX profile this engine lowers.
pub fn is_tex_formula(formula: &MathFormula) -> bool {
    match formula.language.as_deref().map(str::trim) {
        None => true,
        Some(language) => {
            language.is_empty() || language.eq_ignore_ascii_case("tex") || language.eq_ignore_ascii_case("latex")
        }
    }
}

/// Format one column alignment list into the layout alignment list.
pub fn column_alignments(alignments: &[MathColumnAlignment]) -> Vec<Alignment> {
    alignments
        .iter()
        .map(|alignment| match alignment {
            MathColumnAlignment::Left => Alignment::Left,
            MathColumnAlignment::Center => Alignment::Center,
            MathColumnAlignment::Right => Alignment::Right,
            MathColumnAlignment::Spread => Alignment::Center,
        })
        .collect()
}
