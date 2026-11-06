//! Gate G5: no silent font fallbacks.
//!
//! Production mode keeps layout working and reports every consumed fallback as
//! a structured warning diagnostic (`vutex.math.conformance.*`), so a host can
//! count substitutions. Strict mode — the shape CI runs — fails the layout the
//! moment any of the G5 categories is consumed: a missing MATH constant, a
//! missing glyph variant or assembly piece, a missing italic correction or
//! accent attachment, or a glyph the font does not ship at all.
//!
//! The gate runs the shared oracle corpus against the embedded default font in
//! strict mode: with a font whose `MATH` table is fully consumed, the corpus
//! must paint without a single fallback.

use std::fs;

use serde::Deserialize;
use vutex_compose::math::{FontMetricsData, FontStyle, FormulaNode, GlyphKey, LineStyle, MathStyle, SymbolRole};
use vutex_layout::math::{FontConformance, LayoutEngine, layout_formula_source_for, layout_formula_source_with_conformance};
use vutex_text::math::{FontMetricsSystem, MathFontFamily};

#[path = "support/mod.rs"]
mod support;

/// One corpus formula (the same record `oracle_corpus_contract` pins).
#[derive(Debug, Deserialize)]
struct CorpusFormula {
    id: String,
    tex: String,
}

#[derive(Debug, Deserialize)]
struct Corpus {
    formulas: Vec<CorpusFormula>,
}

fn load_corpus() -> Corpus {
    support::ensure_math_fonts();
    let path = support::oracle_records().join("corpus.json");
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{} must be valid corpus JSON: {e}", path.display()))
}

fn symbol(ch: char) -> FormulaNode {
    FormulaNode::Symbol { glyph_key: GlyphKey::from_char(ch, None, FontStyle::Math), role: SymbolRole::Ordinary, span: None }
}

/// The G5 gate: the full corpus lays out in strict mode on both display
/// states, so not one formula consumes a font fallback with the embedded
/// default face.
#[test]
fn corpus_lays_out_in_strict_conformance_mode() {
    let corpus = load_corpus();
    let family = MathFontFamily::builtin_default().expect("embedded font");
    let mut failures = Vec::new();
    for formula in &corpus.formulas {
        for display in [false, true] {
            if let Err(error) =
                layout_formula_source_with_conformance(&formula.tex, display, 16.0, family, FontConformance::Strict)
            {
                failures.push(format!("{} (display={display}): {error}", formula.id));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "strict conformance mode must paint the whole corpus without a fallback:\n{}",
        failures.join("\n")
    );
}

/// Production mode leaves every consumed fallback on the result as a warning
/// with its structured `vutex.math.conformance.*` code, and paints anyway.
#[test]
fn production_mode_counts_fallbacks_as_warnings() {
    // A metrics cache with no font behind it answers everything from the
    // synthetic stand-ins; layout must still succeed and name what it used.
    let font = FontMetricsSystem::new_with_metrics(FontMetricsData::new("default".to_string(), 1000));
    let engine = LayoutEngine::with_facts(font, None).with_base_size(16.0);
    let nodes = vec![
        symbol('x'),
        FormulaNode::Superscript {
            base: Box::new(symbol('y')),
            sup: Box::new(FormulaNode::Number { text: "2".into(), span: None }),
        },
    ];
    let (result, _tree) = engine.layout_formula_result(&nodes, MathStyle::Text).expect("production mode paints anyway");
    let codes: Vec<&str> = result.diagnostics.iter().map(|d| d.code.as_str()).collect();
    assert!(
        codes.iter().any(|code| code.starts_with("vutex.math.conformance.")),
        "every consumed fallback must be reported with a structured code: {codes:?}"
    );
    assert!(
        result
            .diagnostics
            .iter()
            .filter(|d| d.code.starts_with("vutex.math.conformance."))
            .all(|d| d.severity == vutex_compose::Severity::Warning),
        "production fallbacks stay countable warnings, not errors"
    );
}

/// Strict mode refuses the same layout with a structured failure instead of a
/// painted formula.
#[test]
fn strict_mode_fails_on_the_first_fallback_class() {
    let font = FontMetricsSystem::new_with_metrics(FontMetricsData::new("default".to_string(), 1000));
    let engine = LayoutEngine::with_facts(font, None).with_base_size(16.0).with_font_conformance(FontConformance::Strict);
    let nodes = vec![symbol('x')];
    let error = engine
        .layout_formula_result(&nodes, MathStyle::Text)
        .expect_err("strict mode must fail when the font publishes nothing");
    let message = error.to_string();
    assert!(message.contains("vutex.math.conformance."), "the failure names the consumed fallback codes: {message}");
}

/// A delimiter with no `MATH` construction records the missing glyph variant
/// under its own structured code.
#[test]
fn a_delimiter_without_a_construction_is_reported() {
    let font = FontMetricsSystem::new_with_metrics(FontMetricsData::new("default".to_string(), 1000));
    let engine = LayoutEngine::with_facts(font, None).with_base_size(16.0);
    let nodes = vec![FormulaNode::Delimited {
        left: Some(GlyphKey::from_char('(', None, FontStyle::Math)),
        right: Some(GlyphKey::from_char(')', None, FontStyle::Math)),
        content: Box::new(FormulaNode::Fraction {
            num: Box::new(symbol('a')),
            den: Box::new(symbol('b')),
            line: LineStyle::Solid,
        }),
        left_span: None,
        right_span: None,
    }];
    let (result, _tree) = engine.layout_formula_result(&nodes, MathStyle::Display).expect("production mode still paints");
    let codes: Vec<&str> = result.diagnostics.iter().map(|d| d.code.as_str()).collect();
    assert!(
        codes.contains(&"vutex.math.conformance.missing_glyph_variant"),
        "the unsized `(` must be reported as a missing glyph variant: {codes:?}"
    );
    assert!(
        codes.contains(&"vutex.math.conformance.missing_constant"),
        "the fraction constants came from synthetic stand-ins: {codes:?}"
    );
}

/// The embedded default font covers the everyday glyph scope: plain symbols
/// and scripts consume no fallback at all.
#[test]
fn the_embedded_font_consumes_no_fallback_on_plain_formulas() {
    support::ensure_math_fonts();
    let family = MathFontFamily::builtin_default().expect("embedded font");
    for source in ["x^2 + y_1", "\\frac{a}{b}", "\\sqrt[n]{x}", "\\hat{x} + \\int_0^1 f"] {
        let (result, _tree, diagnostics) = layout_formula_source_for(source, false, 14.0, family).expect(source);
        let fallbacks: Vec<_> = diagnostics.iter().filter(|d| d.code.starts_with("vutex.math.conformance.")).collect();
        assert!(
            fallbacks.is_empty(),
            "`{source}` must consume only font-published facts, found {fallbacks:?} (result diagnostics: {:?})",
            result.diagnostics
        );
    }
}
