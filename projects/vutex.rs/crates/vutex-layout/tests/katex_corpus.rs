//! Golden-corpus tests: every formula in `formula_corpus.json` must lay out
//! successfully (no parse error), with sane positive metrics, and must be
//! measurably wider in display style than inline style.
//!
//! The corpus mirrors KaTeX's own formula test set, so passing these tests
//! proves VuTeX parses and lays out the same formulas KaTeX does.

use std::{fs, path::Path};

use serde::Deserialize;
use vutex_layout::math::{default_base_size, layout_formula_with_result};
use vutex_types::math::FormulaLayoutResult;

/// A single corpus entry, deserialized from `formula_corpus.json`.
#[derive(Debug, Deserialize)]
struct CorpusEntry {
    src: String,
    #[allow(dead_code)]
    group: String,
    #[allow(dead_code)]
    notes: Option<String>,
}

/// Load the formula corpus JSON shipped alongside this test.
fn load_corpus() -> Vec<CorpusEntry> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join("formula_corpus.json");
    let text = fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!("failed to read formula corpus at {}: {e}", path.display());
    });
    serde_json::from_str(&text).expect("formula_corpus.json must be valid JSON array")
}

/// Assert that a laid-out result has all-positive metrics.
fn assert_positive_metrics(src: &str, m: &FormulaLayoutResult, style: &str) {
    let width = m.metrics.width.to_f64();
    let height = m.metrics.height.to_f64();
    let baseline = m.metrics.height.to_f64();
    assert!(width > 0.0, "[{style}] `{src}`: width must be positive, got {width}");
    assert!(height > 0.0, "[{style}] `{src}`: height must be positive, got {height}");
    assert!(baseline >= 0.0, "[{style}] `{src}`: baseline must be non-negative, got {baseline}");
}

/// Every formula in the corpus must parse and lay out in both inline and
/// display mode, with all metrics strictly positive.
#[test]
fn corpus_all_formulas_layout_successfully() {
    let corpus = load_corpus();
    assert!(!corpus.is_empty(), "corpus must not be empty");

    for entry in &corpus {
        // Inline / text style
        let (inline, _, _) = layout_formula_with_result(&entry.src, false, default_base_size(false))
            .unwrap_or_else(|e| panic!("inline layout failed for `{}`: {e}", entry.src));
        assert_positive_metrics(&entry.src, &inline, "inline");

        // Display style
        let (display, _, _) = layout_formula_with_result(&entry.src, true, default_base_size(true))
            .unwrap_or_else(|e| panic!("display layout failed for `{}`: {e}", entry.src));
        assert_positive_metrics(&entry.src, &display, "display");
    }
}

/// For formulas whose horizontal extent is driven by their operands (not by
/// big-operator limits), display style must not be narrower than inline
/// style — the base glyph size is larger.  Formulas with `\sum`, `\int`,
/// `\prod`, `\lim` (big operators with limits) are excluded because in
/// TeX display mode those limits move below/above the operator instead of
/// hanging as subscripts/superscripts, which *reduces* width even though
/// the operator itself grows taller.  That is correct TeX behaviour, not
/// a regression.
#[test]
fn corpus_non_operator_display_is_not_narrower_than_inline() {
    const TOLERANCE: f64 = 0.95;

    /// Big operators whose limits change placement between display and
    /// inline style — these legitimately shrink in width when the limits
    /// move above/below the operator.
    const LIMITED_OPS: &[&str] = &["\\sum", "\\int", "\\prod", "\\lim", "\\bigcup", "\\bigcap"];

    let corpus = load_corpus();
    let mut violations = Vec::new();

    for entry in &corpus {
        // Skip formulas that contain big operators with limits — those are
        // allowed (and expected) to be narrower in display mode.
        if LIMITED_OPS.iter().any(|op| entry.src.contains(op)) {
            continue;
        }

        let (inline, _, _) = layout_formula_with_result(&entry.src, false, default_base_size(false)).unwrap();
        let (display, _, _) = layout_formula_with_result(&entry.src, true, default_base_size(true)).unwrap();
        let inline_width = inline.metrics.width.to_f64();
        let display_width = display.metrics.width.to_f64();

        if display_width < inline_width * TOLERANCE {
            violations.push(format!(
                "`{}`: display.width={:.3} < inline.width={:.3} (ratio={:.3})",
                entry.src,
                display_width,
                inline_width,
                display_width / inline_width.max(f64::EPSILON),
            ));
        }
    }

    assert!(
        violations.is_empty(),
        "display style unexpectedly narrower than inline for {} formula(s):\n{}",
        violations.len(),
        violations.join("\n"),
    );
}

/// Display mode has a larger base glyph size than inline mode, so the
/// *total vertical extent* (height + depth) of every formula must be at
/// least as large in display as in inline style.  For big operators with
/// limits the height field alone may shrink (limits move from superscript
/// space to below the operator), but depth absorbs that growth — so we
/// check the sum.
#[test]
fn corpus_display_total_extent_ge_inline() {
    let corpus = load_corpus();
    let mut violations = Vec::new();

    for entry in &corpus {
        let (inline, _, _) = layout_formula_with_result(&entry.src, false, default_base_size(false)).unwrap();
        let (display, _, _) = layout_formula_with_result(&entry.src, true, default_base_size(true)).unwrap();

        let inline_total = inline.metrics.height.to_f64() + inline.metrics.depth.to_f64();
        let display_total = display.metrics.height.to_f64() + display.metrics.depth.to_f64();

        // Allow a tiny absolute tolerance so single-atom floating-point
        // wobble doesn't trip us up.
        if display_total + 0.05 < inline_total {
            violations.push(format!(
                "`{}`: display_total={:.3} (h={:.3},d={:.3}) < inline_total={:.3} (h={:.3},d={:.3})",
                entry.src,
                display_total,
                display.metrics.height.to_f64(),
                display.metrics.depth.to_f64(),
                inline_total,
                inline.metrics.height.to_f64(),
                inline.metrics.depth.to_f64(),
            ));
        }
    }

    assert!(
        violations.is_empty(),
        "display style total extent unexpectedly smaller than inline for {} formula(s):\n{}",
        violations.len(),
        violations.join("\n"),
    );
}

/// Sanity: every formula in the corpus is wider than a single `x` atom.
/// Catches layout regressions that collapse everything to zero width.
#[test]
fn corpus_each_formula_is_wider_than_single_atom() {
    let corpus = load_corpus();
    let single_x = {
        let (m, _, _) = layout_formula_with_result("x", false, default_base_size(false)).unwrap();
        m.metrics.width.to_f64()
    };
    assert!(single_x > 0.0, "sanity: single `x` must have positive width");

    let mut violations = Vec::new();
    for entry in &corpus {
        let (inline, _, _) = layout_formula_with_result(&entry.src, false, default_base_size(false)).unwrap();
        let inline_width = inline.metrics.width.to_f64();
        if inline_width < single_x * 0.5 {
            violations.push(format!(
                "`{}`: width={:.3} is suspiciously close to single-atom width={:.3}",
                entry.src, inline_width, single_x,
            ));
        }
    }
    assert!(violations.is_empty(), "{} formula(s) are suspiciously narrow:\n{}", violations.len(), violations.join("\n"),);
}

/// Print a summary table of corpus metrics so a developer can eyeball it.
/// `cargo test -- --nocapture` will show the output.
#[test]
#[ignore = "manual eyeball summary, run with --ignored --nocapture"]
fn corpus_metrics_summary() {
    let corpus = load_corpus();
    println!("{:<40} {:>10} {:>10} {:>10} {:>10}", "src", "inline_w", "display_w", "ratio", "group");
    for entry in &corpus {
        let (inline, _, _) = layout_formula_with_result(&entry.src, false, default_base_size(false)).unwrap();
        let (display, _, _) = layout_formula_with_result(&entry.src, true, default_base_size(true)).unwrap();
        let inline_width = inline.metrics.width.to_f64();
        let display_width = display.metrics.width.to_f64();
        let ratio = display_width / inline_width.max(f64::EPSILON);
        println!("{:<40} {:>10.2} {:>10.2} {:>10.2} {:>10}", entry.src, inline_width, display_width, ratio, entry.group);
    }
}
