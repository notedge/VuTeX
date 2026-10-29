//! Structured MathML oracle: KaTeX versus VuTeX's semantic lowering.
//!
//! `tests/fixtures/oracle/katex-mathml.json` is produced by
//! `scripts/oracle/emit-katex-mathml.mjs`, which renders every corpus formula
//! with KaTeX — an independent TeX authoring/canonicalization implementation —
//! and stores the resulting Presentation MathML verbatim.
//!
//! This test compares *structure*: which MathML element type each semantic
//! construct became, and how those elements nest. It deliberately does not
//! compare character data, because both engines legitimately pick different
//! codepoints for the same symbol (`∥` vs `‖`) and different spacing encodings.
//! What matters is that `\frac{a+b}{c+d}` is a `mfrac` of two `mrow`s in both,
//! `x^{a_b}` nests `msub` inside `msup` in both, and matrices are `mtable` of
//! `mtr`/`mtd` in both.
//!
//! KaTeX reports no font, size or metrics, and every such field stays `null`
//! here — the oracle makes no geometric claim.

use std::{collections::BTreeMap, fs, path::Path};

use serde::Deserialize;
use vutex_layout::math::parse_formula;
use vutex_render_svg::math::MathmlBackend;
use vutex_types::math::MathSemantics;

/// KaTeX version the fixture must have been generated with.
///
/// It is pinned in `scripts/oracle/package.json`, so an accidental upgrade
/// requires regenerating the fixture instead of silently changing the oracle.
const EXPECTED_KATEX_VERSION: &str = "0.16.22";

/// Elements that wrap other content without changing it: KaTeX's host `<span>`,
/// the `<semantics>` wrapper, and the `<mstyle>` KaTeX puts around every table
/// cell to pin its script level. Their children are spliced into the parent.
const TRANSPARENT_TAGS: &[&str] = &["span", "semantics", "mstyle"];

/// Elements that are pure metadata and disappear entirely.
const DROPPED_TAGS: &[&str] = &["annotation", "mspace"];

#[derive(Debug, Deserialize)]
struct Document {
    schema_version: String,
    source: Source,
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
    status: String,
    status_reason: Option<String>,
}

/// Path of the KaTeX fixture.
fn fixture_path() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join("oracle").join("katex-mathml.json")
}

/// Load the KaTeX fixture.
fn load_document() -> Document {
    let path = fixture_path();
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| {
        panic!("{} must be a KaTeX snapshot; regenerate it with node scripts/oracle/emit-katex-mathml.mjs: {e}", path.display())
    })
}

/// The fixture must be a successful run of the pinned KaTeX, with no fabricated
/// geometric field.
#[test]
fn katex_snapshot_is_honest_about_what_it_measured() {
    let document = load_document();
    assert_eq!(document.schema_version, "v0");
    assert_eq!(document.source.engine, "katex");
    assert_eq!(document.source.status, "available", "the KaTeX snapshot must come from a real run");
    assert_eq!(
        document.source.version.as_deref(),
        Some(EXPECTED_KATEX_VERSION),
        "the snapshot must come from the version pinned in scripts/oracle/package.json"
    );

    assert!(!document.records.is_empty(), "the snapshot must contain records");
    for record in &document.records {
        assert_eq!(record.tex.is_empty(), false, "corpus `{}` lost its source", record.corpus_id);

        // KaTeX emits MathML and nothing else: no font, no size, no metrics.
        // A non-null value here would be a fabricated measurement.
        assert!(record.font.is_none(), "`{}`: KaTeX reports no font", record.corpus_id);
        assert!(record.size.is_none(), "`{}`: KaTeX reports no layout size", record.corpus_id);
        assert!(record.width.is_none(), "`{}`: KaTeX reports no width", record.corpus_id);
        assert!(record.height.is_none(), "`{}`: KaTeX reports no height", record.corpus_id);
        assert!(record.depth.is_none(), "`{}`: KaTeX reports no depth", record.corpus_id);
        assert!(record.axis.is_none(), "`{}`: KaTeX reports no axis", record.corpus_id);
        assert!(record.style_cramped.is_none(), "`{}`: KaTeX reports no style state", record.corpus_id);
        assert!(record.normalized_atoms.is_none(), "`{}`: KaTeX reports no atom model", record.corpus_id);
        assert!(record.ink_bounds.is_none(), "`{}`: KaTeX reports no ink bounds", record.corpus_id);
        assert!(record.rule_thickness.is_none(), "`{}`: KaTeX reports no rule thickness", record.corpus_id);
        assert!(record.delimiter_size.is_none(), "`{}`: KaTeX reports no delimiter geometry", record.corpus_id);
        assert!(record.script_positions.is_none(), "`{}`: KaTeX reports no script positions", record.corpus_id);

        match record.source.status.as_str() {
            "available" => {
                let mathml = record.mathml.as_deref().unwrap_or_else(|| {
                    panic!("`{}` (display={}): an available record must carry MathML", record.corpus_id, record.display_mode)
                });
                assert!(mathml.contains("<math"), "`{}`: the payload must be MathML", record.corpus_id);
            }
            "unsupported" => {
                assert!(
                    record.source.status_reason.as_deref().is_some_and(|reason| !reason.trim().is_empty()),
                    "`{}` (display={}): a rejected formula must record KaTeX's message",
                    record.corpus_id,
                    record.display_mode
                );
                assert!(
                    record.mathml.is_none(),
                    "`{}` (display={}): a rejected formula must not carry a MathML payload",
                    record.corpus_id,
                    record.display_mode
                );
            }
            other => panic!("`{}`: unexpected KaTeX status `{other}`", record.corpus_id),
        }
    }
}

/// Every corpus formula must appear in both styles, so the fixture cannot
/// silently shrink when it is regenerated.
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
        assert_eq!(sorted, vec![false, true], "corpus `{corpus_id}` must be measured in inline and display style");
    }

    let inline_records = document.records.iter().filter(|record| !record.display_mode).count();
    let available =
        document.records.iter().filter(|record| !record.display_mode && record.source.status == "available").count();
    assert_eq!(inline_records, styles.len(), "inline style must have exactly one record per corpus formula");
    assert!(
        available >= styles.len() - 1,
        "KaTeX rejected {} of {} inline formulas, which would make the structure oracle nearly vacuous",
        styles.len() - available,
        styles.len()
    );
}

/// VuTeX's semantic lowering must produce the same MathML element structure as
/// KaTeX for every formula KaTeX accepts.
#[test]
fn structures_match_katex_for_inline_style() {
    let document = load_document();
    let mut compared = 0;

    for record in &document.records {
        // Display style is not compared: KaTeX moves big-operator limits
        // above/below the operator (`munderover`) while VuTeX's semantic tree
        // carries a single `msubsup` shape in both styles, so display mode is a
        // known structural difference rather than a measurement.
        if record.display_mode || record.source.status != "available" {
            continue;
        }
        let katex_mathml = record.mathml.as_deref().expect("available records carry MathML");

        let tree = parse_formula(&record.tex).unwrap_or_else(|e| panic!("corpus `{}` must parse: {e}", record.corpus_id));
        let mut backend = MathmlBackend::new();
        backend.render_semantics(&MathSemantics::from_semantic_tree(&tree));
        let vutex_mathml = backend.finish();

        let expected = normalize(katex_mathml);
        let actual = normalize(&vutex_mathml);
        assert_eq!(
            actual.skeleton(),
            expected.skeleton(),
            "\n`{}` (`{}`) structure differs from KaTeX\n  VuTeX : {}\n  KaTeX : {}\n  KaTeX MathML: {katex_mathml}",
            record.corpus_id,
            record.tex,
            actual.skeleton(),
            expected.skeleton(),
        );
        compared += 1;
    }

    assert!(compared >= 16, "only {compared} formulas were comparable, so the structure oracle is too narrow");
}

// ---------------------------------------------------------------------------
// Minimal MathML structure reader
// ---------------------------------------------------------------------------

/// One MathML element with only the structure this oracle compares.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Element {
    tag: String,
    children: Vec<Node>,
}

/// Raw parse node: an element, or character data.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Node {
    Element(Element),
    Text(String),
}

impl Element {
    /// Render the element tree as `tag(child,child)`, ignoring attributes and
    /// character data.
    fn skeleton(&self) -> String {
        let mut parts = Vec::new();
        for child in &self.children {
            if let Node::Element(element) = child {
                parts.push(element.skeleton());
            }
        }
        if parts.is_empty() {
            return self.tag.clone();
        }
        format!("{}({})", self.tag, parts.join(","))
    }
}

/// Reduce a MathML document to the root structure both engines can be compared
/// on: the children of `<math>`, with non-structural wrappers removed, `mstyle`
/// spliced away, empty groups dropped and single-child `mrow`s collapsed.
///
/// Every rule here removes a difference that is presentation-only, and each one
/// is documented where it applies. None of them can merge two different
/// constructs: an `mfrac` never becomes an `msqrt`.
fn normalize(mathml: &str) -> Element {
    let parsed = parse_nodes(mathml);
    let math = find_math(&parsed).unwrap_or_else(|| panic!("payload has no `<math>` element: {mathml}"));
    Element { tag: "math".to_string(), children: simplify(&math.children).into_iter().map(Node::Element).collect() }
}

/// Locate the first `math` element in a parsed document.
fn find_math(nodes: &[Node]) -> Option<&Element> {
    for node in nodes {
        if let Node::Element(element) = node {
            if element.tag == "math" {
                return Some(element);
            }
            if let Some(found) = find_math(&element.children) {
                return Some(found);
            }
        }
    }
    None
}

/// Apply the normalization rules to one child list.
fn simplify(nodes: &[Node]) -> Vec<Element> {
    let mut output = Vec::new();
    for node in nodes {
        let Node::Element(element) = node
        else {
            // Character data carries no structure. The one thing it can
            // express is a whitespace-only `mtext`, handled below.
            continue;
        };

        if DROPPED_TAGS.contains(&element.tag.as_str()) {
            continue;
        }

        // Wrappers that only add host markup or style scoping to their
        // children; both engines insert and omit them differently.
        if TRANSPARENT_TAGS.contains(&element.tag.as_str()) {
            output.extend(simplify(&element.children));
            continue;
        }

        // A whitespace-only text atom is spacing. VuTeX renders explicit math
        // space as `mspace` (already dropped), KaTeX renders `\,` as `mtext`
        // holding a thin space.
        if element.tag == "mtext" && is_whitespace_text(element) {
            continue;
        }

        let simplified_children: Vec<Node> = simplify(&element.children).into_iter().map(Node::Element).collect();
        let mut simplified = Element { tag: element.tag.clone(), children: simplified_children };
        if simplified.tag == "mrow" {
            // An empty group is an authoring artifact (`&=` leaves one in
            // KaTeX); it contributes nothing to the formula.
            if simplified.children.is_empty() {
                continue;
            }
            // A group around a single child is pure grouping, and both engines
            // insert or omit it freely.
            if simplified.children.len() == 1 {
                if let Node::Element(only) = simplified.children.pop().expect("length checked above") {
                    output.push(only);
                }
                continue;
            }
        }
        output.push(simplified);
    }
    output
}

/// Whether an element carries only blank character data — a spacing atom
/// rather than a piece of the formula.
fn is_whitespace_text(element: &Element) -> bool {
    element.children.iter().all(|child| match child {
        Node::Text(text) => text.trim().is_empty(),
        Node::Element(_) => false,
    })
}

/// Parse tags and character data into a node tree. Only element names and
/// nesting matter, so attribute values may contain any character except `>`.
fn parse_nodes(text: &str) -> Vec<Node> {
    let mut stack: Vec<(String, Vec<Node>)> = vec![(String::new(), Vec::new())];
    let mut rest = text;

    while let Some(open) = rest.find('<') {
        if open > 0 {
            push_text(stack.last_mut().expect("root frame"), &rest[..open]);
        }
        let after = &rest[open..];
        let close = after.find('>').unwrap_or_else(|| panic!("unterminated tag in MathML: {after}"));
        let tag_body = &after[1..close];
        rest = &after[close + 1..];

        if tag_body.starts_with('/') {
            let name = tag_body[1..].trim().to_ascii_lowercase();
            let (popped, children) = stack.pop().expect("closing tag with no open element");
            assert_eq!(popped, name, "mismatched MathML tag `</{name}>` closes `<{popped}>`");
            let parent = stack.last_mut().expect("closing tag at root");
            parent.1.push(Node::Element(Element { tag: name, children }));
            continue;
        }
        if tag_body.starts_with('!') || tag_body.starts_with('?') {
            // Doctype / processing instruction: nothing structural.
            continue;
        }

        let self_closing = tag_body.ends_with('/');
        let name =
            tag_body.trim_end_matches('/').split([' ', '\t', '\n', '\r']).next().unwrap_or_default().to_ascii_lowercase();
        if self_closing {
            stack.last_mut().expect("root frame").1.push(Node::Element(Element { tag: name, children: Vec::new() }));
        }
        else {
            stack.push((name, Vec::new()));
        }
    }

    let (root_tag, nodes) = stack.pop().expect("balanced parse must leave the root frame");
    assert_eq!(root_tag, "", "unclosed MathML element `<{root_tag}>`");
    nodes
}

/// Push character data as a text node, keeping raw text so blank detection can
/// tell a thin space from real content.
fn push_text(frame: &mut (String, Vec<Node>), text: &str) {
    frame.1.push(Node::Text(text.to_string()));
}
