//! LaTeX geometry oracle status: a probe record, not a measurement.
//!
//! `tests/fixtures/oracle/latex-status.json` is written by
//! `scripts/oracle/probe-latex.mjs`, which asks the machine whether a TeX
//! engine exists. LaTeX is the only independent typesetter that reports the
//! box metrics (`width`, `height`, `depth`) VuTeX needs, so when it is absent
//! the geometry oracle has no evidence at all.
//!
//! This test exists to keep that absence honest: the document must say
//! `blocked` with the observed probe output, every record must stay `null`, and
//! the documented procedure must name the commands and hooks a real harness
//! would use. A future run with TeX installed may flip the status to
//! `available`; the assertions then require real numbers instead of nulls.

use std::{collections::BTreeMap, fs, path::Path};

use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Document {
    schema_version: String,
    source: Source,
    procedure: Vec<String>,
    probes: Vec<Probe>,
    amsmath: Amsmath,
    records: Vec<Record>,
}

#[derive(Debug, Deserialize)]
struct Source {
    engine: String,
    version: Option<String>,
    command: String,
    status: String,
    status_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Probe {
    name: String,
    found: bool,
    exit_code: Option<i32>,
    version_line: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Amsmath {
    found: bool,
    version: Option<String>,
    reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Record {
    corpus_id: String,
    tex: String,
    display_mode: bool,
    source: RecordSource,
    font: Option<serde_json::Value>,
    size: Option<f64>,
    normalized_atoms: Option<serde_json::Value>,
    style_cramped: Option<bool>,
    width: Option<f64>,
    height: Option<f64>,
    depth: Option<f64>,
    axis: Option<f64>,
    ink_bounds: Option<serde_json::Value>,
    rule_thickness: Option<f64>,
    delimiter_size: Option<f64>,
    script_positions: Option<serde_json::Value>,
    mathml: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RecordSource {
    engine: String,
    status: String,
    status_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Corpus {
    formulas: Vec<CorpusFormula>,
}

#[derive(Debug, Deserialize)]
struct CorpusFormula {
    id: String,
}

/// Directory of all oracle fixtures.
fn fixture_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join("oracle")
}

/// Load the LaTeX status fixture.
fn load_document() -> Document {
    let path = fixture_dir().join("latex-status.json");
    let text = fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!("failed to read {}: {e}\n  regenerate it with node scripts/oracle/probe-latex.mjs", path.display())
    });
    serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("{} must be a probe snapshot, not a hand-written record: {e}", path.display()))
}

/// Identifier of every corpus formula, in fixture order.
fn corpus_ids() -> Vec<String> {
    let path = fixture_dir().join("corpus.json");
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    let corpus: Corpus = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{} must be valid JSON: {e}", path.display()));
    corpus.formulas.into_iter().map(|formula| formula.id).collect()
}

/// The status must agree with what the probes actually saw: a blocked document
/// needs observed evidence, an available one needs a version and real numbers.
#[test]
fn latex_status_matches_what_the_probe_observed() {
    let document = load_document();
    assert_eq!(document.schema_version, "v0");
    assert_eq!(document.source.engine, "latex");
    assert_eq!(document.source.command, "node scripts/oracle/probe-latex.mjs");

    let lualatex = document
        .probes
        .iter()
        .find(|probe| probe.name == "lualatex")
        .expect("the probe must report on `lualatex`, the engine the oracle depends on");

    match document.source.status.as_str() {
        "blocked" => {
            assert!(document.source.version.is_none(), "a blocked status must not claim an engine version");
            let reason =
                document.source.status_reason.as_deref().expect("a blocked status must explain itself in `status_reason`");
            assert!(!reason.trim().is_empty(), "the blocked reason must not be blank");

            // A blocked status is only credible when the probe observed either a
            // missing engine or a failing one, or when the reason names the
            // missing harness instead of pretending the engine is absent.
            if !lualatex.found {
                assert!(
                    lualatex.error.as_deref().is_some_and(|error| !error.trim().is_empty()),
                    "a missing `lualatex` must leave the spawn error in the probe"
                );
            }
            else {
                assert_ne!(lualatex.exit_code, Some(0), "a working `lualatex` cannot be reported as blocked");
            }

            let lualatex_works = lualatex.found && lualatex.exit_code == Some(0);
            assert!(
                !lualatex_works || reason.to_ascii_lowercase().contains("harness"),
                "a running `lualatex` must be reported as a missing harness, not as a missing engine"
            );
        }
        "available" => {
            assert!(
                document.source.version.as_deref().is_some_and(|version| !version.trim().is_empty()),
                "an available status must record the engine version that produced the numbers"
            );
            assert!(document.source.status_reason.is_none(), "an available status must not carry a blocked reason");
            assert!(
                lualatex.version_line.as_deref().is_some_and(|line| !line.trim().is_empty()),
                "an available status must record the probed engine version line"
            );
            assert!(
                document.records.iter().any(|record| record.width.is_some()),
                "an available status must carry at least one real width"
            );
        }
        other => panic!("unexpected LaTeX oracle status `{other}`"),
    }

    if !document.amsmath.found {
        assert!(
            document.amsmath.reason.as_deref().is_some_and(|reason| !reason.trim().is_empty()),
            "a missing `amsmath` must be explained"
        );
    }
    else {
        assert!(document.amsmath.version.is_some(), "a found `amsmath` must have its declared version recorded");
    }
}

/// The fixture must document the probe and the measurement procedure, so the
/// blocked oracle is a described gap rather than an unexplained absence.
#[test]
fn procedure_documents_the_probe_and_generation_flow() {
    let document = load_document();
    assert!(document.procedure.len() >= 4, "the procedure must describe probe, box measuring, conversion and limits");

    for (index, step) in document.procedure.iter().enumerate() {
        assert!(!step.trim().is_empty(), "procedure step {} is blank", index + 1);
    }

    let joined = document.procedure.join("\n").to_ascii_lowercase();
    for needle in ["lualatex", "amsmath", "showbox", "pdfsavepos", "em"] {
        assert!(joined.contains(needle), "the procedure must mention `{needle}`");
    }
}

/// A blocked document may list records, but every one of them must stay null:
/// no geometry may be filled in by hand to make the oracle look satisfied.
#[test]
fn blocked_records_cover_the_corpus_without_inventing_geometry() {
    let document = load_document();
    let ids = corpus_ids();

    let mut styles: BTreeMap<&str, Vec<bool>> = BTreeMap::new();
    for record in &document.records {
        styles.entry(record.corpus_id.as_str()).or_default().push(record.display_mode);
    }
    let recorded: Vec<String> = styles.keys().map(|id| id.to_string()).collect();
    assert_eq!(recorded.len(), ids.len(), "the fixture must cover every corpus formula exactly once");
    for id in &ids {
        assert!(recorded.contains(id), "the fixture is missing corpus formula `{id}`");
    }

    for (corpus_id, modes) in &styles {
        let mut sorted = modes.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, vec![false, true], "corpus `{corpus_id}` must be recorded in inline and display style");
    }

    let blocked = document.source.status == "blocked";
    for record in &document.records {
        assert!(!record.tex.is_empty(), "corpus `{}` lost its source", record.corpus_id);
        assert_eq!(record.source.engine, "latex");

        if !blocked {
            continue;
        }
        assert_eq!(
            record.source.status, "blocked",
            "`{}`: a blocked document cannot report measured records",
            record.corpus_id
        );
        assert!(
            record.source.status_reason.as_deref().is_some_and(|reason| !reason.trim().is_empty()),
            "`{}`: a blocked record must carry the probe evidence",
            record.corpus_id
        );
        assert!(record.font.is_none(), "`{}`: no font can be claimed without an engine", record.corpus_id);
        assert!(record.width.is_none(), "`{}`: no width can be claimed without an engine", record.corpus_id);
        assert!(record.height.is_none(), "`{}`: no height can be claimed without an engine", record.corpus_id);
        assert!(record.depth.is_none(), "`{}`: no depth can be claimed without an engine", record.corpus_id);
        assert!(record.axis.is_none(), "`{}`: no axis can be claimed without an engine", record.corpus_id);
        assert!(record.size.is_none(), "`{}`: no size can be claimed without an engine", record.corpus_id);
        assert!(record.style_cramped.is_none(), "`{}`: no style state can be claimed", record.corpus_id);
        assert!(record.normalized_atoms.is_none(), "`{}`: no atom model can be claimed", record.corpus_id);
        assert!(record.ink_bounds.is_none(), "`{}`: no ink bounds can be claimed", record.corpus_id);
        assert!(record.rule_thickness.is_none(), "`{}`: no rule thickness can be claimed", record.corpus_id);
        assert!(record.delimiter_size.is_none(), "`{}`: no delimiter size can be claimed", record.corpus_id);
        assert!(record.script_positions.is_none(), "`{}`: no script positions can be claimed", record.corpus_id);
        assert!(record.mathml.is_none(), "`{}`: LaTeX emits no MathML", record.corpus_id);
    }

    if blocked {
        // A blocked oracle must be visible in the one number that matters: there
        // is no measured width anywhere in the fixture.
        assert!(
            document.records.iter().all(|record| record.width.is_none()),
            "a blocked LaTeX oracle must not contain a measured width"
        );
    }
}
