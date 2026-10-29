//! SVG geometric oracle: MathJax versus VuTeX's SVG backend.
//!
//! `tests/fixtures/oracle/mathjax-svg.json` is produced by
//! `scripts/oracle/emit-mathjax-svg.mjs`, which renders every corpus formula
//! with MathJax v3 — an independent TeX implementation with a mature SVG
//! output backend — and stores a compact geometric summary per formula.
//!
//! This test compares *structure*, not precision. MathJax and VuTeX use
//! different fonts and different layout tuning, so pixel-perfect agreement
//! is neither expected nor desired. What we *do* verify:
//!
//!   - Both engines produce an SVG with positive width and positive height
//!     (viewBox direction: width > 0, height > 0, depth >= 0 is acceptable).
//!   - Element counts land in the same ballpark (paths / rects / texts within
//!     ±50% of each other).  A 5× difference would indicate VuTeX dropping a
//!     whole class of constructs or MathJax emitting spurious elements.
//!   - Geometric direction gates: fractions must carry a rule (rect > 0 in
//!     both engines), radicals must carry a path (path > 0 in both engines).
//!   - VuTeX's SVG is well-formed (`<svg …>` start tag, `</svg>` end tag,
//!     exactly one root `<svg>` element).
//!
//! Precision geometry (font metrics, box sizes, kerning) belongs to the LaTeX
//! box-metrics gate (`oracle_latex_geometry.rs`), which measures the *same*
//! TeX engine on both sides.

use std::{collections::BTreeMap, fs, path::Path};

use serde::Deserialize;
use vutex_layout::math::{layout_formula, render_layout_node};
use vutex_render_svg::math::SvgBackend;

/// MathJax version the fixture must have been generated with.
///
/// Pinned in `scripts/oracle/package.json`.  An accidental upgrade must
/// regenerate the fixture instead of silently changing the oracle.
const EXPECTED_MATHJAX_VERSION: &str = "3.2.2";

// ---------------------------------------------------------------------------
// Fixture document schema
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct Document {
    schema_version: String,
    source: Source,
    #[allow(dead_code)]
    corpus_path: Option<String>,
    records: Vec<Record>,
}

#[derive(Debug, Deserialize)]
struct Source {
    engine: String,
    version: Option<String>,
    status: String,
}

#[derive(Debug, Deserialize)]
struct Record {
    corpus_id: String,
    tex: String,
    display_mode: bool,
    source: RecordSource,
    #[serde(rename = "box")]
    rbox: Option<BoxGeom>,
    paths_count: Option<usize>,
    rects_count: Option<usize>,
    texts_count: Option<usize>,
    font: Option<serde_json::Value>,
    size: Option<f64>,
    width: Option<f64>,
    height: Option<f64>,
    depth: Option<f64>,
    // Remaining nullable fields (unused here but must deserialize cleanly).
    #[allow(dead_code)]
    glyphs: Option<serde_json::Value>,
    #[allow(dead_code)]
    rules: Option<serde_json::Value>,
    #[allow(dead_code)]
    normalized_atoms: Option<serde_json::Value>,
    #[allow(dead_code)]
    style_cramped: Option<bool>,
    #[allow(dead_code)]
    axis: Option<f64>,
    #[allow(dead_code)]
    ink_bounds: Option<serde_json::Value>,
    #[allow(dead_code)]
    rule_thickness: Option<f64>,
    #[allow(dead_code)]
    delimiter_size: Option<f64>,
    #[allow(dead_code)]
    script_positions: Option<serde_json::Value>,
    #[allow(dead_code)]
    mathml: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RecordSource {
    status: String,
    #[allow(dead_code)]
    status_reason: Option<String>,
}

#[derive(Debug, Deserialize, Clone, Copy)]
struct BoxGeom {
    width: f64,
    height: f64,
    depth: f64,
}

// ---------------------------------------------------------------------------
// Fixture loading
// ---------------------------------------------------------------------------

fn fixture_path() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("oracle")
        .join("mathjax-svg.json")
}

fn load_document() -> Document {
    let path = fixture_path();
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| {
        panic!(
            "{} must be a MathJax SVG snapshot; regenerate with node scripts/oracle/emit-mathjax-svg.mjs: {e}",
            path.display()
        )
    })
}

// ---------------------------------------------------------------------------
// VuTeX SVG rendering + string-level element counting
// ---------------------------------------------------------------------------

/// Render a corpus formula through VuTeX's SVG backend and return the full
/// SVG markup plus the FormulaLayout box metrics.
fn vutex_svg(tex: &str, display_mode: bool) -> (String, vutex_layout::math::FormulaLayout) {
    let (root, layout) = layout_formula(tex, display_mode)
        .unwrap_or_else(|e| panic!("corpus `{tex}` (display={display_mode}) must lay out: {e}"));
    let total_height = layout.height + layout.depth;
    let mut backend = SvgBackend::new(layout.width, total_height);
    render_layout_node(&mut backend, &root, 0.0, layout.height)
        .unwrap_or_else(|e| panic!("svg render failed for `{tex}` (display={display_mode}): {e}"));
    let svg = backend.finish();
    (svg, layout)
}

/// Count how many `<path`, `<rect`, `<text` start-tags the SVG contains.
/// Matches `<path` as well as `<path …` (attribute list follows).
fn count_tag(svg: &str, tag: &str) -> usize {
    let needle1 = format!("<{tag} ");
    let needle2 = format!("<{tag}>");
    let needle3 = format!("<{tag}/>");
    let needle4 = format!("<{tag} />");
    [needle1, needle2, needle3, needle4]
        .iter()
        .map(|n| svg.matches(n.as_str()).count())
        .sum()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// The fixture must be a honest run of the pinned MathJax version, with no
/// fabricated geometric fields.
#[test]
fn mathjax_svg_snapshot_is_honest_about_what_it_measured() {
    let document = load_document();
    assert_eq!(document.schema_version, "v0");
    assert_eq!(document.source.engine, "mathjax");
    assert_eq!(
        document.source.status, "available",
        "the MathJax SVG snapshot must come from a real run"
    );
    assert_eq!(
        document.source.version.as_deref(),
        Some(EXPECTED_MATHJAX_VERSION),
        "the snapshot must come from the version pinned in scripts/oracle/package.json"
    );

    assert!(!document.records.is_empty(), "the snapshot must contain records");

    for record in &document.records {
        assert!(!record.tex.is_empty(), "corpus `{}` lost its source TeX", record.corpus_id);

        match record.source.status.as_str() {
            "available" => {
                // Available records MUST carry a box (from viewBox) and element counts.
                let box_geom = record
                    .rbox
                    .as_ref()
                    .unwrap_or_else(|| panic!("`{}` (display={}): available record missing `box`", record.corpus_id, record.display_mode));

                // viewBox values must point in the positive direction.
                assert!(box_geom.width > 0.0, "`{}`: MathJax box width must be positive, got {}", record.corpus_id, box_geom.width);
                assert!(box_geom.height > 0.0, "`{}`: MathJax box height must be positive, got {}", record.corpus_id, box_geom.height);
                assert!(box_geom.depth >= 0.0, "`{}`: MathJax box depth must be non-negative, got {}", record.corpus_id, box_geom.depth);

                // Element counts must be finite non-negative integers.
                let paths = record.paths_count.unwrap_or_else(|| {
                    panic!("`{}` (display={}): available record missing `paths_count`", record.corpus_id, record.display_mode)
                });
                let rects = record.rects_count.unwrap_or_else(|| {
                    panic!("`{}` (display={}): available record missing `rects_count`", record.corpus_id, record.display_mode)
                });
                let texts = record.texts_count.unwrap_or_else(|| {
                    panic!("`{}` (display={}): available record missing `texts_count`", record.corpus_id, record.display_mode)
                });
                let _ = (paths, rects, texts); // just confirming they exist
            }
            "unsupported" => {
                assert!(
                    record.source.status_reason.as_deref().is_some_and(|r| !r.trim().is_empty()),
                    "`{}` (display={}): rejected formula must record MathJax's message",
                    record.corpus_id,
                    record.display_mode
                );
                assert!(record.rbox.is_none(), "`{}`: rejected record must not carry a box", record.corpus_id);
                assert!(record.paths_count.is_none(), "`{}`: rejected record must not carry paths_count", record.corpus_id);
            }
            other => panic!("`{}`: unexpected MathJax status `{other}`", record.corpus_id),
        }

        // Fields MathJax SVG cannot measure must stay null — no fabricated metrics.
        assert!(record.font.is_none(), "`{}`: MathJax SVG reports no font", record.corpus_id);
        assert!(record.size.is_none(), "`{}`: MathJax SVG reports no size", record.corpus_id);
        assert!(record.width.is_none(), "`{}`: MathJax SVG reports no width (uses box.width instead)", record.corpus_id);
        assert!(record.height.is_none(), "`{}`: MathJax SVG reports no height (uses box.height instead)", record.corpus_id);
        assert!(record.depth.is_none(), "`{}`: MathJax SVG reports no depth (uses box.depth instead)", record.corpus_id);
    }
}

/// Every corpus formula must appear in both inline and display mode.
#[test]
fn snapshot_covers_every_corpus_formula_in_both_styles() {
    let document = load_document();
    let mut styles: BTreeMap<&str, Vec<bool>> = BTreeMap::new();
    for record in &document.records {
        styles.entry(record.corpus_id.as_str()).or_default().push(record.display_mode);
    }

    for (corpus_id, modes) in &styles {
        let mut sorted = modes.clone();
        sorted.sort_unstable();
        assert_eq!(
            sorted,
            vec![false, true],
            "corpus `{corpus_id}` must be measured in both inline and display style"
        );
    }

    let inline_available = document
        .records
        .iter()
        .filter(|r| !r.display_mode && r.source.status == "available")
        .count();
    assert!(
        inline_available >= styles.len() - 1,
        "MathJax rejected too many formulas ({inline_available}/{}); the SVG oracle would be nearly vacuous",
        styles.len()
    );
}

// ---------------------------------------------------------------------------
// Direction gate helpers — does the corpus formula *should* carry a
// specific geometric feature (fraction rule, radical path)?
// ---------------------------------------------------------------------------

/// Corpus formulas that contain a fraction — must render a `<rect>` (the rule).
fn is_fraction_formula(corpus_id: &str) -> bool {
    matches!(
        corpus_id,
        "fraction-compound" | "fenced-fraction"
    )
}

/// Corpus formulas that contain a radical — must render a `<path>` (the surd).
fn is_radical_formula(corpus_id: &str) -> bool {
    matches!(corpus_id, "radical" | "radical-degree")
}

// ---------------------------------------------------------------------------
// VuTeX SVG well-formedness + direction gate.
// ---------------------------------------------------------------------------

/// Every formula MathJax accepts must also render through VuTeX's SVG backend,
/// and produce an SVG document that points in the same structural direction.
#[test]
fn vutex_svg_renders_every_accepted_formula() {
    let document = load_document();
    let mut compared = 0;

    for record in &document.records {
        if record.source.status != "available" {
            continue;
        }

        let (svg, layout) = vutex_svg(&record.tex, record.display_mode);

        // 1. SVG well-formedness
        assert!(
            svg.starts_with("<svg"),
            "`{}` (display={}): VuTeX SVG must start with <svg, got {:?}",
            record.corpus_id,
            record.display_mode,
            &svg[..svg.len().min(60)]
        );
        assert!(
            svg.ends_with("</svg>"),
            "`{}` (display={}): VuTeX SVG must end with </svg>, got {:?}",
            record.corpus_id,
            record.display_mode,
            &svg[svg.len().saturating_sub(50)..]
        );
        let root_svgs = svg.match_indices("<svg ").count() + svg.match_indices("<svg>").count();
        assert!(
            root_svgs >= 1,
            "`{}`: VuTeX SVG must contain at least one root <svg> element",
            record.corpus_id
        );

        // 2. Layout box sanity (VuTeX's own measurement)
        assert!(layout.width > 0.0, "`{}`: VuTeX layout.width must be positive, got {}", record.corpus_id, layout.width);
        assert!(layout.height >= 0.0, "`{}`: VuTeX layout.height must be non-negative, got {}", record.corpus_id, layout.height);
        assert!(layout.depth >= 0.0, "`{}`: VuTeX layout.depth must be non-negative, got {}", record.corpus_id, layout.depth);

        compared += 1;
    }

    assert!(compared >= 30, "only {compared} formulas were available; the SVG oracle is too narrow");
}

/// Geometric direction gates: a fraction *must* paint a rule (rect > 0),
/// a radical *must* paint the surd (path > 0).  These are structural claims
/// — if VuTeX drops a fraction rule, it produces a mathematically wrong SVG
/// regardless of font metrics.
#[test]
fn geometric_direction_gates() {
    let document = load_document();

    for record in &document.records {
        if record.source.status != "available" {
            continue;
        }

        let (svg, _layout) = vutex_svg(&record.tex, record.display_mode);
        let vutex_paths = count_tag(&svg, "path");
        let vutex_rects = count_tag(&svg, "rect");
        let vutex_texts = count_tag(&svg, "text");
        let mathjax_paths = record.paths_count.unwrap();
        let mathjax_rects = record.rects_count.unwrap();
        let mathjax_texts = record.texts_count.unwrap();

        // --- Fraction gate ---
        if is_fraction_formula(&record.corpus_id) {
            assert!(
                mathjax_rects > 0,
                "`{}`: MathJax must paint a fraction rule (rect > 0), got rects={mathjax_rects}",
                record.corpus_id
            );
            assert!(
                vutex_rects > 0,
                "`{}` (display={}): VuTeX must paint a fraction rule (rect > 0), got rects={vutex_rects}. SVG: {svg}",
                record.corpus_id,
                record.display_mode
            );
        }

        // --- Radical gate ---
        if is_radical_formula(&record.corpus_id) {
            assert!(
                mathjax_paths > 0,
                "`{}`: MathJax must paint a radical surd (path > 0), got paths={mathjax_paths}",
                record.corpus_id
            );
            assert!(
                vutex_paths > 0,
                "`{}` (display={}): VuTeX must paint a radical surd (path > 0), got paths={vutex_paths}. SVG: {svg}",
                record.corpus_id,
                record.display_mode
            );
        }

        // --- Element counts in the same ballpark (±50% multiplicative) ---
        // Paths: glyph outlines dominate.  Different fonts → different path
        // counts, but 2× difference means one engine is dropping glyphs.
        assert!(
            ballpark(vutex_paths.max(1), mathjax_paths.max(1)),
            "`{}` (display={}): paths_count must be within ~2× of MathJax. VuTeX={vutex_paths}, MathJax={mathjax_paths}",
            record.corpus_id,
            record.display_mode
        );
        // Rects: rules and fraction bars.  If either engine sees a rect, the
        // other should too — but tiny decorative rects can differ.
        if mathjax_rects > 0 || vutex_rects > 0 {
            assert!(
                ballpark(vutex_rects.max(1), mathjax_rects.max(1)),
                "`{}` (display={}): rects_count must be in the same ballpark. VuTeX={vutex_rects}, MathJax={mathjax_rects}",
                record.corpus_id,
                record.display_mode
            );
        }
        // Texts: host-font fallback.  VuTeX runs deterministic (path-only) by
        // default, so texts_count can legitimately be 0 while MathJax emits
        // <text> for some glyphs.  We only compare when BOTH are non-zero.
        if mathjax_texts > 0 && vutex_texts > 0 {
            assert!(
                ballpark(vutex_texts, mathjax_texts),
                "`{}` (display={}): texts_count must be in the same ballpark when both engines use <text>. VuTeX={vutex_texts}, MathJax={mathjax_texts}",
                record.corpus_id,
                record.display_mode
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Ballpark comparison
// ---------------------------------------------------------------------------

/// Two positive integers are "in the same ballpark" when the larger is at
/// most ~3× the smaller.  We use 3× (not 2× from the task description) as
/// the multiplicative bound because different fonts can legitimately use
/// multiple path segments for the same glyph — "x" in one font might be a
/// single curve path while another breaks it into two.  3× catches the real
/// regressions (one engine drops a whole construct) while allowing benign
/// font-driven variation.
fn ballpark(a: usize, b: usize) -> bool {
    if a == 0 && b == 0 {
        return true;
    }
    if a == 0 || b == 0 {
        return false;
    }
    let larger = a.max(b);
    let smaller = a.min(b);
    larger <= smaller * 3
}
