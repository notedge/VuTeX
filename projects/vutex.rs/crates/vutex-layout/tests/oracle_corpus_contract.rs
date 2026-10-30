//! Oracle corpus and fixture-schema contract.
//!
//! Every external-oracle fixture in `tests/fixtures/oracle/` is keyed by the
//! shared corpus in `tests/fixtures/oracle/corpus.json` and must speak the
//! record shape documented in `projects/vutex.ts/examples/oracle/fixture-schema.json`. This test
//! pins both halves of that contract so a regenerated fixture cannot silently
//! drift away from the corpus or drop a measurement field.
//!
//! It is a contract test, not an oracle: it asserts nothing about geometry.
//! The claims about real engines live in the per-engine oracle tests.

use std::{collections::BTreeSet, fs, path::Path};

use serde::Deserialize;
use serde_json::Value;

/// The formula coverage the oracle corpus must keep, in corpus order.
///
/// Dropping one of these narrows how much of the formula space external engines
/// can contradict us on, so the list is asserted instead of documented only.
const REQUIRED_CORPUS_IDS: &[&str] = &[
    "sum-of-atoms",
    "relation",
    "fraction-compound",
    "script-nested",
    "radical",
    "radical-degree",
    "sum-limits",
    "integral-limits",
    "fenced-fraction",
    "fenced-norm",
    "accent-hat",
    "accent-bar",
    "matrix",
    "cases",
    "aligned",
    "alignedat",
    "split",
];

/// Per-record fields whose absence must be recorded as `null`, never as zero.
const NULLABLE_RECORD_FIELDS: &[&str] = &[
    "normalized_atoms",
    "style_cramped",
    "width",
    "height",
    "depth",
    "axis",
    "ink_bounds",
    "rule_thickness",
    "delimiter_size",
    "script_positions",
];

/// Fixture directory of the `vutex-layout` crate.
fn fixture_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join("oracle")
}

/// Path of the shared oracle record schema in the repository's script directory.
fn schema_path() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("..")
        .join("projects")
        .join("vutex.ts")
        .join("examples")
        .join("oracle")
        .join("fixture-schema.json")
}

/// One corpus formula.
#[derive(Debug, Deserialize)]
struct CorpusFormula {
    id: String,
    tex: String,
    group: String,
    #[serde(default)]
    notes: Option<String>,
}

/// The shared corpus document.
#[derive(Debug, Deserialize)]
struct Corpus {
    schema_version: String,
    #[serde(default)]
    notes: Vec<String>,
    formulas: Vec<CorpusFormula>,
}

/// Load the shared corpus.
fn load_corpus() -> Corpus {
    let path = fixture_dir().join("corpus.json");
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{} must be valid corpus JSON: {e}", path.display()))
}

/// The corpus must contain exactly the declared coverage, with no duplicates.
#[test]
fn corpus_covers_the_required_formula_set() {
    let corpus = load_corpus();
    assert_eq!(corpus.schema_version, "v0", "corpus schema version must stay on the `/v0` contract namespace");
    assert!(!corpus.notes.is_empty(), "corpus must document how oracles consume it");

    let ids: Vec<&str> = corpus.formulas.iter().map(|f| f.id.as_str()).collect();
    let unique: BTreeSet<&str> = ids.iter().copied().collect();
    assert_eq!(unique.len(), ids.len(), "corpus ids must be unique, found a duplicate in {ids:?}");

    for required in REQUIRED_CORPUS_IDS {
        assert!(unique.contains(required), "corpus lost required formula `{required}`");
    }
    assert_eq!(
        ids,
        REQUIRED_CORPUS_IDS.to_vec(),
        "corpus order defines fixture order, so it must match the declared coverage list"
    );

    for formula in &corpus.formulas {
        assert!(!formula.tex.trim().is_empty(), "corpus formula `{}` has no source", formula.id);
        assert!(!formula.group.trim().is_empty(), "corpus formula `{}` has no group", formula.id);
        assert!(
            formula.notes.as_deref().is_some_and(|n| !n.trim().is_empty()),
            "corpus formula `{}` must say what the entry covers",
            formula.id
        );
    }
}

/// Every corpus formula must be accepted by the VuTeX parser.
///
/// A corpus entry VuTeX cannot parse would make the KaTeX and LaTeX oracles
/// compare structures against a formula the engine under test never sees.
#[test]
fn corpus_formulas_parse_in_vutex() {
    let corpus = load_corpus();
    for formula in &corpus.formulas {
        vutex_layout::math::parse_formula(&formula.tex)
            .unwrap_or_else(|e| panic!("corpus formula `{}` does not parse: {e}\n  tex: {}", formula.id, formula.tex));
    }
}

/// The record schema must enumerate every oracle measurement as nullable.
///
/// `null` is the only honest encoding of "this engine does not report the
/// field", so a schema that types a field as `number` would force generators to
/// write a fabricated `0`.
#[test]
fn schema_keeps_every_measurement_nullable() {
    let path = schema_path();
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    let schema: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{} must be valid JSON: {e}", path.display()));

    assert_eq!(schema["type"], "object", "a record is a JSON object");
    let required: Vec<&str> =
        schema["required"].as_array().expect("schema must list required keys").iter().filter_map(Value::as_str).collect();
    for key in ["schema_version", "corpus_id", "tex", "display_mode", "source"] {
        assert!(required.contains(&key), "record schema must require `{key}`");
    }

    let properties = schema["properties"].as_object().expect("record schema must declare properties");
    for field in NULLABLE_RECORD_FIELDS {
        let spec = properties
            .get(*field)
            .unwrap_or_else(|| panic!("record schema lost the `{field}` measurement, so no oracle can report it"));
        assert!(
            accepts_null(spec),
            "`{field}` must accept null: absent engine capability is recorded as null, never as a fabricated value"
        );
    }

    let definitions = schema["definitions"].as_object().expect("record schema must define its sub-objects");
    for definition in ["source", "font", "atom", "bbox", "scriptPosition"] {
        assert!(definitions.contains_key(definition), "record schema must define `{definition}`");
    }

    let status = &definitions["source"]["properties"]["status"];
    let variants: Vec<&str> =
        status["enum"].as_array().expect("source.status must be an enum").iter().filter_map(Value::as_str).collect();
    assert_eq!(
        variants,
        vec!["available", "blocked", "unsupported"],
        "source.status must keep blocked and unsupported as first-class outcomes"
    );
}

/// Whether a property spec allows JSON null, either directly or through `oneOf`.
fn accepts_null(spec: &Value) -> bool {
    match &spec["type"] {
        Value::Array(types) => types.iter().any(|t| t == "null"),
        Value::String(t) => t == "null",
        _ => spec["oneOf"].as_array().is_some_and(|branches| branches.iter().any(|b| b["type"] == "null")),
    }
}
