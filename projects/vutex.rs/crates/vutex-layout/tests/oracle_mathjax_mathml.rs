//! Structured MathML oracle: MathJax versus VuTeX's semantic lowering.
//!
//! `tests/fixtures/oracle/mathjax-mathml.json` is produced by
//! `projects/vutex.ts/examples/oracle/emit-mathjax-mathml.mjs`, which renders every corpus formula
//! with MathJax v3 — a second independent TeX → MathML implementation — and
//! stores the resulting Presentation MathML verbatim.
//!
//! This test is a **second** structure oracle alongside the KaTeX one. Two
//! engines means the oracle is no longer "MathJax == VuTeX" but
//! "both MathJax and KaTeX expose the same core shape VuTeX lowers to". That
//! catches silent regressions in either comparison direction.
//!
//! The comparison is deliberately tolerant. MathJax and KaTeX may legitimately
//! differ in:
//!   - extra `<mrow>` wrapping,
//!   - `<mtable>` columnalign / columnspacing strings,
//!   - how `\lVert` / `\rVert` is encoded,
//!   - how ams environments (matrix, aligned, split) spread empty cells.
//! What *is* compared is that every formula's core semantic element tree
//! exists: a fraction is an `mfrac` of two groups, a sum is an `msubsup` or
//! `munderover`, a matrix is an `mtable` of `mtr` / `mtd`, and so on. Root
//! `<math>` presence, `display="block"` propagation, and MathML namespace are
//! also verified.
//!
//! MathJax reports no font, size or metrics, and every such field stays `null`
//! here — the oracle makes no geometric claim.

use std::{collections::BTreeMap, fs, path::Path};

use serde::Deserialize;
use vutex_layout::math::parse_formula;
use vutex_render_svg::math::MathmlBackend;
use vutex_types::math::MathSemantics;

/// MathJax version the fixture must have been generated with.
///
/// It is pinned in `projects/vutex.ts/examples/oracle/package.json`, so an accidental upgrade
/// requires regenerating the fixture instead of silently changing the oracle.
const EXPECTED_MATHJAX_VERSION: &str = "3.2.2";

/// Wrapper tags that add no structural information: MathJax's `<semantics>`
/// (KaTeX also emits this, so it lives in both oracles), and MathJax-specific
/// `<mstyle>` scoping around table cells. Their children are spliced into the
/// parent so they never cause a spurious difference.
const TRANSPARENT_TAGS: &[&str] = &["span", "semantics", "mstyle"];

/// Tags that are pure metadata or disappear entirely. `<annotation>` is KaTeX's
/// TeX source echo, `<mspace>` is explicit spacing that both engines produce
/// differently, `<annotation-xml>` is MathJax's secondary MathML serialization
/// that we never compare.
const DROPPED_TAGS: &[&str] = &["annotation", "annotation-xml", "mspace"];

/// Core MathML semantic elements we care about. Each one represents a distinct
/// TeX construct (fraction, radical, superscript, table…) and must survive the
/// tolerant normalization unscathed.
const CORE_SEMANTIC_TAGS: &[&str] = &[
    "mfrac",
    "msqrt",
    "mroot",
    "msup",
    "msub",
    "msubsup",
    "munder",
    "mover",
    "munderover",
    "mtable",
    "mtr",
    "mtd",
    "mtext",
    "mrow",
    "mo",
    "mi",
    "mn",
    "mfenced",
];

// ---------------------------------------------------------------------------
// Schema mirror (shared shape with the KaTeX oracle — same fixture-schema.json)
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

/// Path of the MathJax fixture.
fn fixture_path() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join("oracle").join("mathjax-mathml.json")
}

/// Load the MathJax fixture, panicking with a regenerate hint if missing.
fn load_document() -> Document {
    let path = fixture_path();
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| {
        panic!(
            "{} must be a MathJax snapshot; regenerate it with node projects/vutex.ts/examples/oracle/emit-mathjax-mathml.mjs: {e}",
            path.display()
        )
    })
}

// ---------------------------------------------------------------------------
// Fixture honesty contract (MathJax reports no geometry — every numeric field
// must be null so the oracle does not fabricate evidence)
// ---------------------------------------------------------------------------

/// The fixture must be a successful run of the pinned MathJax, with no
/// fabricated geometric field.
#[test]
fn mathjax_snapshot_is_honest_about_what_it_measured() {
    let document = load_document();
    assert_eq!(document.schema_version, "v0");
    assert_eq!(document.source.engine, "mathjax");
    assert_eq!(document.source.status, "available", "the MathJax snapshot must come from a real run");
    assert_eq!(
        document.source.version.as_deref(),
        Some(EXPECTED_MATHJAX_VERSION),
        "the snapshot must come from the version pinned in projects/vutex.ts/examples/oracle/package.json"
    );

    assert!(!document.records.is_empty(), "the snapshot must contain records");
    for record in &document.records {
        assert!(!record.tex.is_empty(), "corpus `{}` lost its source", record.corpus_id);

        // MathJax emits MathML and nothing else: no font, no size, no metrics.
        assert!(record.font.is_none(), "`{}`: MathJax reports no font", record.corpus_id);
        assert!(record.size.is_none(), "`{}`: MathJax reports no layout size", record.corpus_id);
        assert!(record.width.is_none(), "`{}`: MathJax reports no width", record.corpus_id);
        assert!(record.height.is_none(), "`{}`: MathJax reports no height", record.corpus_id);
        assert!(record.depth.is_none(), "`{}`: MathJax reports no depth", record.corpus_id);
        assert!(record.axis.is_none(), "`{}`: MathJax reports no axis", record.corpus_id);
        assert!(record.style_cramped.is_none(), "`{}`: MathJax reports no style state", record.corpus_id);
        assert!(record.normalized_atoms.is_none(), "`{}`: MathJax reports no atom model", record.corpus_id);
        assert!(record.ink_bounds.is_none(), "`{}`: MathJax reports no ink bounds", record.corpus_id);
        assert!(record.rule_thickness.is_none(), "`{}`: MathJax reports no rule thickness", record.corpus_id);
        assert!(record.delimiter_size.is_none(), "`{}`: MathJax reports no delimiter geometry", record.corpus_id);
        assert!(record.script_positions.is_none(), "`{}`: MathJax reports no script positions", record.corpus_id);

        match record.source.status.as_str() {
            "available" => {
                let mathml = record.mathml.as_deref().unwrap_or_else(|| {
                    panic!("`{}` (display={}): an available record must carry MathML", record.corpus_id, record.display_mode)
                });
                assert!(mathml.contains("<math"), "`{}`: the payload must be MathML", record.corpus_id);
                assert!(mathml.contains("xmlns"), "`{}`: MathJax MathML must declare the MathML namespace", record.corpus_id);
            }
            "unsupported" => {
                assert!(
                    record.source.status_reason.as_deref().is_some_and(|r| !r.trim().is_empty()),
                    "`{}` (display={}): a rejected formula must record MathJax's message",
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
            other => panic!("`{}`: unexpected MathJax status `{other}`", record.corpus_id),
        }
    }
}

// ---------------------------------------------------------------------------
// Coverage contract: every corpus formula must appear in both display styles
// ---------------------------------------------------------------------------

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
        "MathJax rejected {} of {} inline formulas, which would make the structure oracle nearly vacuous",
        styles.len() - available,
        styles.len()
    );
}

// ---------------------------------------------------------------------------
// Root-level MathML sanity
// ---------------------------------------------------------------------------

/// Every available record must produce a `<math>` root with the correct
/// `display` attribute matching the requested style.
#[test]
fn root_math_element_has_correct_display_attribute() {
    let document = load_document();
    for record in &document.records {
        if record.source.status != "available" {
            continue;
        }
        let mathml = record.mathml.as_deref().expect("available records carry MathML");
        let math_open_tag = extract_math_open_tag(mathml)
            .unwrap_or_else(|| panic!("`{}`: MathJax MathML has no `<math>` root tag: {mathml}", record.corpus_id));

        let has_display_block = math_open_tag.contains("display=\"block\"");
        if record.display_mode {
            assert!(
                has_display_block,
                "`{}` (display=true): root <math> must carry display=\"block\", got: {math_open_tag}",
                record.corpus_id
            );
        }
        else {
            assert!(
                !has_display_block,
                "`{}` (display=false): root <math> must NOT carry display=\"block\", got: {math_open_tag}",
                record.corpus_id
            );
        }
    }
}

/// Extract the opening `<math …>` tag string from a MathML document.
fn extract_math_open_tag(mathml: &str) -> Option<&str> {
    let start = mathml.find("<math")?;
    let rest = &mathml[start..];
    let end = rest.find('>')?;
    Some(&rest[..=end])
}

// ---------------------------------------------------------------------------
// Tolerant structure presence: each corpus formula must expose its core
// semantic element in MathJax's output (we check *presence*, not exact nesting
// skeleton — MathJax and KaTeX disagree on mrow wrapping, empty cells, etc.).
// ---------------------------------------------------------------------------

/// Map each corpus formula to the core MathML element **families** we expect
/// to see somewhere in its normalized tree. Each inner list is an OR group:
/// any one of those tags satisfies the requirement. This handles legitimate
/// engine differences — MathJax uses `munderover` for inline large operators
/// where KaTeX/VuTeX use `msubsup`, and both are correct Presentation MathML.
fn expected_core_tag_families(corpus_id: &str) -> &'static [&'static [&'static str]] {
    match corpus_id {
        "sum-of-atoms" => &[],
        "relation" => &[],
        "fraction-compound" => &[&["mfrac"]],
        "script-nested" => &[&["msup"], &["msub"]],
        "radical" => &[&["msqrt"]],
        "radical-degree" => &[&["mroot"]],
        // Large operator with sub+super: MathJax uses munderover in BOTH styles,
        // KaTeX/VuTeX use msubsup in inline and munderover in display. Either
        // tag is acceptable.
        "sum-limits" => &[&["munderover", "msubsup"], &["mo"]],
        "integral-limits" => &[&["msubsup"], &["msup"]],
        "fenced-fraction" => &[&["mfrac"], &["mo"]],
        "fenced-norm" => &[&["mo"]],
        "accent-hat" => &[&["mover"]],
        "accent-bar" => &[&["mover"]],
        "matrix" => &[&["mtable"], &["mtr"], &["mtd"]],
        "cases" => &[&["mtable"], &["mtr"], &["mtd"], &["mo"]],
        "aligned" => &[&["mtable"], &["mtr"], &["mtd"]],
        "alignedat" => &[&["mtable"], &["mtr"], &["mtd"]],
        "split" => &[&["mtable"], &["mtr"], &["mtd"]],
        other => panic!("unknown corpus id `{other}` in expected_core_tag_families"),
    }
}

/// Each available formula's normalized MathML tree must contain the core
/// semantic element **families** that correspond to its TeX source. Each
/// family is an OR group (any one satisfies the check), so the oracle stays
/// tolerant of legitimate engine differences (munderover vs msubsup, etc.).
#[test]
fn core_semantic_elements_exist_in_mathjax_output() {
    let document = load_document();
    let mut checked = 0;

    for record in &document.records {
        if record.source.status != "available" {
            continue;
        }
        let mathml = record.mathml.as_deref().expect("available records carry MathML");
        let normalized = normalize(mathml);
        let skeleton = normalized.skeleton();

        // Always at least a <math> root
        assert!(
            skeleton.starts_with("math"),
            "`{}` (display={}): normalized tree must start with math, got {skeleton}",
            record.corpus_id,
            record.display_mode
        );

        let families = expected_core_tag_families(&record.corpus_id);

        // Collect the set of core semantic tags present in the MathJax output
        // (using the same normalize rules as the cross-engine comparison).
        let present_tags: Vec<String> = collect_core_tags(&normalized);

        for family in families {
            let found = family.iter().any(|tag| present_tags.iter().any(|pt| pt == tag));
            assert!(
                found,
                "`{}` (display={}): expected one of {:?} in MathJax MathML\n  present core tags: {:?}\n  skeleton: {skeleton}\n  raw: {mathml}",
                record.corpus_id, record.display_mode, family, present_tags,
            );
        }
        checked += 1;
    }

    assert!(checked >= 16, "only {checked} formulas were checked, so the structure oracle is too narrow");
}

// ---------------------------------------------------------------------------
// Tolerant cross-engine shape comparison: MathJax and VuTeX must agree on the
// core construct present in each formula. We compare the presence set of
// semantic tags rather than exact nesting skeleton, because MathJax differs
// from KaTeX (and potentially from VuTeX) in benign wrapping mrows and table
// spacing attributes.
// ---------------------------------------------------------------------------

/// For each inline formula MathJax accepts, VuTeX's MathML backend must also
/// produce the same set of core semantic tags.
#[test]
fn vutex_core_elements_match_mathjax_for_inline_style() {
    let document = load_document();
    let mut compared = 0;

    for record in &document.records {
        // Display mode is not compared: large-operator limits move above/below
        // in MathJax output (munderover vs msubsup), which is a known
        // structural difference rather than a measurement failure.
        if record.display_mode || record.source.status != "available" {
            continue;
        }
        let mathjax_mathml = record.mathml.as_deref().expect("available records carry MathML");

        let tree = parse_formula(&record.tex).unwrap_or_else(|e| panic!("corpus `{}` must parse: {e}", record.corpus_id));
        let mut backend = MathmlBackend::new();
        backend.render_semantics(&MathSemantics::from_semantic_tree(&tree));
        let vutex_mathml = backend.finish();

        let mathjax_normalized = normalize(mathjax_mathml);
        let vutex_normalized = normalize(&vutex_mathml);

        let mathjax_tags = collect_core_tags(&mathjax_normalized);
        let vutex_tags = collect_core_tags(&vutex_normalized);

        // For each core semantic family MathJax exposes, VuTeX must expose at
        // least one member of the same family. Families capture legitimate
        // engine differences — munderover ↔ msubsup for large operators with
        // both sub- and super-scripts, munder ↔ msub for operators with a
        // subscript only, and mover ↔ msup for operators with a superscript
        // only. Any other tag name must match exactly.
        for tag in &mathjax_tags {
            let present = vutex_tags.iter().any(|vt| {
                match tag.as_str() {
                    // Large operator with sub+super: MathJax uses munderover
                    // even inline; KaTeX/VuTeX use msubsup inline.
                    "munderover" | "msubsup" => matches!(vt.as_str(), "munderover" | "msubsup"),
                    // Operator with subscript only.
                    "munder" | "msub" => matches!(vt.as_str(), "munder" | "msub"),
                    // Operator with superscript only.
                    "mover" | "msup" => matches!(vt.as_str(), "mover" | "msup"),
                    // Everything else must match exactly.
                    other => vt.as_str() == other,
                }
            });
            assert!(
                present,
                "`{}` (`{}`): MathJax produces `<{tag}>` but VuTeX exposes no equivalent tag.\n  MathJax skeleton: {}\n  VuTeX skeleton: {}\n  MathJax MathML: {mathjax_mathml}\n  VuTeX MathML: {vutex_mathml}",
                record.corpus_id,
                record.tex,
                mathjax_normalized.skeleton(),
                vutex_normalized.skeleton(),
            );
        }
        compared += 1;
    }

    assert!(compared >= 16, "only {compared} formulas were comparable, so the structure oracle is too narrow");
}

// ---------------------------------------------------------------------------
// Minimal MathML structure reader (tolerant variant)
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
    /// character data — same skeleton format as the KaTeX oracle so both tests
    /// can be read the same way.
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

/// Collect the set of core semantic tags present anywhere in a normalized
/// MathML tree. Used for tolerant presence checks.
fn collect_core_tags(element: &Element) -> Vec<String> {
    let mut tags = Vec::new();
    let tag_lower = element.tag.as_str();
    if CORE_SEMANTIC_TAGS.contains(&tag_lower) && !tags.contains(&element.tag) {
        tags.push(element.tag.clone());
    }
    for child in &element.children {
        if let Node::Element(child_elem) = child {
            let child_tags = collect_core_tags(child_elem);
            for ct in child_tags {
                if !tags.contains(&ct) {
                    tags.push(ct);
                }
            }
        }
    }
    tags
}

/// Reduce a MathML document to the root structure both engines can be compared
/// on: the children of `<math>`, with non-structural wrappers removed, style
/// scoping spliced away, empty groups dropped and single-child `mrow`s
/// collapsed.
///
/// Every rule here removes a difference that is presentation-only, and each
/// one is documented where it applies. None of them can merge two different
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
            // Character data carries no structure.
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

        // A whitespace-only text atom is spacing.
        if element.tag == "mtext" && is_whitespace_text(element) {
            continue;
        }

        let simplified_children: Vec<Node> = simplify(&element.children).into_iter().map(Node::Element).collect();
        let mut simplified = Element { tag: element.tag.clone(), children: simplified_children };
        if simplified.tag == "mrow" {
            // An empty group is an authoring artifact; it contributes nothing.
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
