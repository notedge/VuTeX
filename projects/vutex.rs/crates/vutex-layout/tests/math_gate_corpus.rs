//! Self-check for the auditable math gate corpus.
//!
//! `fixtures/math_gate_corpus.json` is the shared corpus behind the semantic,
//! geometry and PDF gates. This test keeps the corpus honest on its own: it
//! proves the coverage the gate claims (every required construct group is
//! present), that every record declares a well-formed style and invariant set,
//! that `layout` records really lay out with positive metrics, and that `error`
//! records really fail with a parse error.

use std::{fs, path::Path};

use serde::Deserialize;
use vutex_layout::math::layout_formula;
use vutex_types::math::MathErrorKind;

/// Invariants every `layout` record is checked against by the gate.
const BASE_INVARIANTS: &[&str] = &["glyph_count", "ink_in_viewbox", "no_text", "positive_metrics"];
/// Extra invariants a record may request on top of [`BASE_INVARIANTS`].
const EXTRA_INVARIANTS: &[&str] =
    &["display_fraction_height", "script_fraction_shrinks", "delimiter_covers_body", "rule_for_bar", "parse_error"];

/// One formula record in the gate corpus.
#[derive(Debug, Deserialize)]
struct GateCase {
    src: String,
    group: String,
    style: String,
    expected: String,
    diagnostics: bool,
    invariants: Vec<String>,
    body: Option<String>,
}

/// The corpus document, with its provenance and layout.
#[derive(Debug, Deserialize)]
struct GateCorpus {
    corpus: String,
    version: u32,
    source: String,
    cases: Vec<GateCase>,
}

/// Load the gate corpus shipped alongside this test.
fn load_corpus() -> GateCorpus {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join("math_gate_corpus.json");
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read gate corpus at {}: {e}", path.display()));
    serde_json::from_str(&text).expect("math_gate_corpus.json must be a valid corpus document")
}

/// Whether the record is laid out with the display pipeline.
fn is_display(style: &str) -> bool {
    style == "display"
}

#[test]
fn corpus_declares_its_provenance_and_version() {
    let corpus = load_corpus();
    assert_eq!(corpus.corpus, "vutex-math-gate", "unexpected corpus name");
    assert_eq!(corpus.version, 1, "corpus shape is versioned and bumped on a breaking record change");
    assert!(!corpus.source.trim().is_empty(), "the corpus must name where its formulas come from");
    assert!(corpus.cases.len() >= 30, "the gate corpus must stay broad, found {} cases", corpus.cases.len());
}

#[test]
fn corpus_covers_every_required_construct_group() {
    const REQUIRED: &[&str] = &[
        "arithmetic",
        "relation",
        "fraction",
        "radical",
        "scripts",
        "limits",
        "accent",
        "delimiter",
        "matrix",
        "cases",
        "aligned",
        "split",
        "inline",
        "display",
        "error",
    ];

    let corpus = load_corpus();
    let groups: Vec<&str> = corpus.cases.iter().map(|case| case.group.as_str()).collect();
    for group in REQUIRED {
        assert!(groups.contains(group), "the corpus is missing the `{group}` group");
    }

    // The matrix group must cover the three delimiter styles as separate cases.
    for environment in ["pmatrix", "bmatrix", "Vmatrix"] {
        assert!(
            corpus.cases.iter().any(|case| case.group == "matrix" && case.src.contains(environment)),
            "the matrix group is missing `{environment}`"
        );
    }

    // Fractions must be exercised in display, text and script style.
    for style in ["display", "text", "script"] {
        assert!(
            corpus.cases.iter().any(|case| case.group == "fraction" && case.style == style),
            "the fraction group is missing the `{style}` style"
        );
    }

    let errors = corpus.cases.iter().filter(|case| case.group == "error").count();
    assert_eq!(errors, 2, "the corpus must carry exactly two error inputs, found {errors}");
}

#[test]
fn corpus_records_declare_well_formed_expectations() {
    let corpus = load_corpus();

    for case in &corpus.cases {
        let label = format!("`{}` ({})", case.src, case.group);
        assert!(matches!(case.style.as_str(), "display" | "text" | "script"), "{label}: unknown style `{}`", case.style);
        assert!(matches!(case.expected.as_str(), "layout" | "error"), "{label}: unknown expectation `{}`", case.expected);
        assert!(!case.invariants.is_empty(), "{label}: a record must name the invariants it locks");

        for invariant in &case.invariants {
            assert!(
                BASE_INVARIANTS.contains(&invariant.as_str()) || EXTRA_INVARIANTS.contains(&invariant.as_str()),
                "{label}: unknown invariant `{invariant}`"
            );
        }

        if case.invariants.iter().any(|invariant| invariant == "delimiter_covers_body") {
            assert!(case.body.is_some(), "{label}: `delimiter_covers_body` needs a `body` to compare against");
        }

        // The gate only makes sense if the record says whether ink is expected.
        if case.expected == "error" {
            assert!(case.diagnostics, "{label}: a parse error is surfaced as a diagnostic");
        }
        else {
            assert!(!case.diagnostics, "{label}: a layout record must not expect diagnostics");
        }
    }
}

#[test]
fn corpus_layout_records_produce_positive_metrics() {
    let corpus = load_corpus();

    for case in corpus.cases.iter().filter(|case| case.expected == "layout") {
        let display = is_display(&case.style);
        let (_, metrics) =
            layout_formula(&case.src, display).unwrap_or_else(|e| panic!("`{}` ({}) must lay out: {e}", case.src, case.style));

        assert!(
            metrics.width.is_finite() && metrics.width > 0.0,
            "`{}`: width must be positive, got {}",
            case.src,
            metrics.width
        );
        assert!(
            metrics.height.is_finite() && metrics.height > 0.0,
            "`{}`: height must be positive, got {}",
            case.src,
            metrics.height
        );
        // Descender-free atoms (`x^2`, `\hat{x}`) legitimately sit on the
        // baseline with no depth, so the gate forbids negative depth instead of
        // demanding ink below every formula.
        assert!(
            metrics.depth.is_finite() && metrics.depth >= 0.0,
            "`{}`: depth must not be negative, got {}",
            case.src,
            metrics.depth
        );

        if let Some(body) = &case.body {
            layout_formula(body, display)
                .unwrap_or_else(|e| panic!("`{}`: delimiter body `{body}` must lay out: {e}", case.src));
        }
    }
}

#[test]
fn corpus_error_records_really_fail_to_parse() {
    let corpus = load_corpus();

    for case in corpus.cases.iter().filter(|case| case.expected == "error") {
        let error = layout_formula(&case.src, is_display(&case.style)).expect_err(&format!("`{}` must be rejected", case.src));
        assert!(
            matches!(*error.kind, MathErrorKind::ParseError { .. }),
            "`{}` must fail as a parse error, got {:?}",
            case.src,
            error
        );
    }
}
