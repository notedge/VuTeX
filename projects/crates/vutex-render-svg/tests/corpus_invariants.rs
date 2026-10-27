//! Semantic and geometry invariants for the shared math gate corpus.
//!
//! These checks need no reference engine: they read the layout tree and the SVG
//! the real outline pipeline emits, and prove that every formula in
//! `vutex-layout/tests/fixtures/math_gate_corpus.json` paints ink, paints it
//! inside a positive viewport, never falls back to a host-font `<text>` run and
//! keeps the structural relations the corpus records commit to (display
//! fractions, script fractions, stretched delimiters, the `\bar` rule).

use serde::Deserialize;
use vutex_layout::math::{
    FormulaLayout, LayoutNode, collect_layout_with_diagnostics, layout_formula, render_layout_node,
};
use vutex_render_svg::math::SvgBackend;

/// One formula record in the gate corpus.
#[derive(Debug, Deserialize)]
struct GateCase {
    src: String,
    group: String,
    style: String,
    expected: String,
    #[allow(dead_code)]
    diagnostics: bool,
    invariants: Vec<String>,
    body: Option<String>,
}

/// The corpus document shipped with `vutex-layout`.
#[derive(Debug, Deserialize)]
struct GateCorpus {
    corpus: String,
    cases: Vec<GateCase>,
}

/// Every invariant the gate implements.
const IMPLEMENTED: &[&str] = &[
    "glyph_count",
    "ink_in_viewbox",
    "no_text",
    "positive_metrics",
    "display_fraction_height",
    "script_fraction_shrinks",
    "delimiter_covers_body",
    "rule_for_bar",
    "parse_error",
];

fn load_corpus() -> GateCorpus {
    let corpus: GateCorpus =
        serde_json::from_str(include_str!("../../vutex-layout/tests/fixtures/math_gate_corpus.json"))
            .expect("math_gate_corpus.json must parse");
    assert_eq!(corpus.corpus, "vutex-math-gate", "the gate reads the shared math corpus");
    corpus
}

fn is_display(case: &GateCase) -> bool {
    case.style == "display"
}

/// Lay the case out and render it through the strict, path-only SVG backend.
///
/// Strict mode fails the render as soon as a glyph would need a host-font
/// `<text>` run, so a returned SVG already carries outlines only.
fn render(case: &GateCase) -> (LayoutNode, FormulaLayout, String) {
    let (root, metrics) = layout_formula(&case.src, is_display(case))
        .unwrap_or_else(|e| panic!("`{}` ({}) must lay out: {e}", case.src, case.style));
    let mut backend = SvgBackend::new_deterministic(metrics.width, metrics.height + metrics.depth);
    render_layout_node(&mut backend, &root, 0.0, metrics.height)
        .unwrap_or_else(|e| panic!("`{}` must render as outlines: {e}", case.src));
    (root, metrics, backend.finish())
}

/// Number of outlined glyph nodes in a layout tree.
fn glyph_nodes(node: &LayoutNode) -> usize {
    match node {
        LayoutNode::Glyph(_) => 1,
        LayoutNode::HBox(hbox) => hbox.children.iter().map(glyph_nodes).sum(),
        LayoutNode::VBox(vbox) => vbox.children.iter().map(glyph_nodes).sum(),
        _ => 0,
    }
}

/// Number of filled rule boxes in a layout tree.
fn rule_nodes(node: &LayoutNode) -> usize {
    match node {
        LayoutNode::Rule { .. } => 1,
        LayoutNode::HBox(hbox) => hbox.children.iter().map(rule_nodes).sum(),
        LayoutNode::VBox(vbox) => vbox.children.iter().map(rule_nodes).sum(),
        _ => 0,
    }
}

/// Number of `<g>` groups that contain no element at all.
fn empty_groups(svg: &str) -> usize {
    let mut empty = 0;
    let mut stack: Vec<bool> = Vec::new();
    let mut cursor = 0;
    while let Some(open) = svg[cursor..].find('<').map(|i| i + cursor) {
        let Some(length) = svg[open..].find('>')
        else {
            break;
        };
        let tag = &svg[open + 1..open + length];
        cursor = open + length + 1;
        if tag == "g" || tag.starts_with("g ") {
            if let Some(parent) = stack.last_mut() {
                *parent = true;
            }
            stack.push(false);
        }
        else if tag == "/g" {
            if let Some(has_child) = stack.pop() {
                if !has_child {
                    empty += 1;
                }
            }
        }
        else if tag.starts_with("path") || tag.starts_with("rect") || tag.starts_with("text") {
            if let Some(parent) = stack.last_mut() {
                *parent = true;
            }
        }
    }
    empty
}

/// Read the root `viewBox` as `(min_x, min_y, width, height)`.
fn view_box(svg: &str) -> (f64, f64, f64, f64) {
    let tag_end = svg.find('>').expect("root tag");
    let needle = "viewBox=\"";
    let start = svg[..tag_end].find(needle).expect("root viewBox") + needle.len();
    let end = start + svg[start..].find('"').expect("viewBox terminator");
    let values: Vec<f64> = svg[start..end].split_whitespace().map(|value| value.parse().expect("viewBox number")).collect();
    assert_eq!(values.len(), 4, "viewBox needs four numbers: {svg}");
    (values[0], values[1], values[2], values[3])
}

/// Read one attribute out of a single tag body.
fn attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let needle = format!("{name}=\"");
    let start = tag.find(&needle)? + needle.len();
    let end = start + tag[start..].find('"')?;
    Some(&tag[start..end])
}

/// Read the `(x, y)` numbers of the first `prefix` transform in `text`.
fn transform_pair(text: &str, prefix: &str) -> Option<(f64, f64)> {
    let start = text.find(prefix)? + prefix.len();
    let end = start + text[start..].find(')')?;
    let (x, y) = text[start..end].split_once(',')?;
    Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
}

/// Axis-aligned ink bounds of everything the document paints, in canvas pixels.
///
/// Group translations accumulate, and every path applies its own
/// `translate(x, y) scale(sx, sy)` on top in the same order the browser uses.
fn ink_bounds(svg: &str) -> Option<(f64, f64, f64, f64)> {
    let mut bounds: Option<(f64, f64, f64, f64)> = None;
    let mut include = |x: f64, y: f64| {
        if !x.is_finite() || !y.is_finite() {
            return;
        }
        bounds = Some(match bounds {
            None => (x, y, x, y),
            Some((min_x, min_y, max_x, max_y)) => (min_x.min(x), min_y.min(y), max_x.max(x), max_y.max(y)),
        });
    };

    let mut saved: Vec<(f64, f64)> = Vec::new();
    let mut offset = (0.0, 0.0);
    let mut cursor = 0;
    while let Some(open) = svg[cursor..].find('<').map(|i| i + cursor) {
        let Some(length) = svg[open..].find('>')
        else {
            break;
        };
        let tag = &svg[open + 1..open + length];
        cursor = open + length + 1;
        if tag == "g" || tag.starts_with("g ") {
            saved.push(offset);
            if let Some((x, y)) = transform_pair(tag, "translate(") {
                offset = (offset.0 + x, offset.1 + y);
            }
        }
        else if tag == "/g" {
            offset = saved.pop().unwrap_or((0.0, 0.0));
        }
        else if tag.starts_with("path ") {
            let d = attribute(tag, "d").expect("path data");
            let transform = attribute(tag, "transform").unwrap_or_default();
            let (tx, ty) = transform_pair(transform, "translate(").unwrap_or((0.0, 0.0));
            let (sx, sy) = transform_pair(transform, "scale(").unwrap_or((1.0, 1.0));
            for (px, py) in d_points(d) {
                include(offset.0 + tx + sx * px, offset.1 + ty + sy * py);
            }
        }
        else if tag.starts_with("rect ") {
            let x = attribute(tag, "x").expect("rect x").parse::<f64>().expect("rect x number");
            let y = attribute(tag, "y").expect("rect y").parse::<f64>().expect("rect y number");
            let width = attribute(tag, "width").expect("rect width").parse::<f64>().expect("rect width number");
            let height = attribute(tag, "height").expect("rect height").parse::<f64>().expect("rect height number");
            include(offset.0 + x, offset.1 + y);
            include(offset.0 + x + width, offset.1 + y + height);
        }
    }
    bounds
}

/// Coordinate pairs of an absolute path whose `d` only uses `M`/`L`/`Q`/`C`.
fn d_points(d: &str) -> Vec<(f64, f64)> {
    assert!(
        d.chars().all(|c| c.is_whitespace() || "MLQCZ,".contains(c) || c.is_ascii_digit() || c == '.' || c == '-' || c == '+'),
        "unexpected path operator in {d}"
    );
    let mut points = Vec::new();
    let mut numbers: Vec<f64> = Vec::new();
    let mut operator = 'M';
    let mut token = String::new();
    for c in d.chars() {
        if c.is_ascii_alphabetic() {
            push_number(&mut token, &mut numbers);
            flush_numbers(operator, &mut numbers, &mut points);
            operator = c;
        }
        else if c.is_whitespace() || c == ',' {
            push_number(&mut token, &mut numbers);
        }
        else {
            token.push(c);
        }
    }
    push_number(&mut token, &mut numbers);
    flush_numbers(operator, &mut numbers, &mut points);
    points
}

fn push_number(token: &mut String, numbers: &mut Vec<f64>) {
    if !token.is_empty() {
        numbers.push(token.parse().expect("path number"));
        token.clear();
    }
}

fn flush_numbers(operator: char, numbers: &mut Vec<f64>, points: &mut Vec<(f64, f64)>) {
    let arity = match operator {
        'M' | 'L' | 'T' => 2,
        'Q' | 'S' => 4,
        'C' => 6,
        _ => {
            numbers.clear();
            return;
        }
    };
    for chunk in numbers.chunks_exact(arity) {
        for pair in chunk.chunks(2) {
            points.push((pair[0], pair[1]));
        }
    }
    numbers.clear();
}

/// Total vertical extent (`height + depth`) of a formula.
fn total_extent(source: &str, display: bool) -> f64 {
    let (_, metrics) = layout_formula(source, display).unwrap_or_else(|e| panic!("`{source}` must lay out: {e}"));
    metrics.height + metrics.depth
}

/// The gate must implement every invariant the corpus claims, or the fixture
/// and the checks drift apart silently.
#[test]
fn gate_implements_every_declared_invariant() {
    let corpus = load_corpus();
    assert!(!corpus.cases.is_empty(), "the gate corpus must not be empty");

    for case in &corpus.cases {
        for invariant in &case.invariants {
            assert!(IMPLEMENTED.contains(&invariant.as_str()), "`{}`: the gate has no check for `{invariant}`", case.src);
        }
    }
}

/// Every layout case paints outlined ink, in a strict backend, with no empty
/// group and without a host-font `<text>` fallback.
#[test]
fn every_case_paints_outlines_without_host_text() {
    let corpus = load_corpus();
    let mut violations = Vec::new();

    for case in corpus.cases.iter().filter(|case| case.expected == "layout") {
        let (root, metrics, svg) = render(case);
        let label = format!("`{}` ({})", case.src, case.style);

        let glyphs = glyph_nodes(&root);
        if glyphs == 0 {
            violations.push(format!("{label}: the layout tree holds no glyph"));
        }
        if empty_groups(&svg) != 0 {
            violations.push(format!("{label}: {} empty <g> group(s):\n{svg}", empty_groups(&svg)));
        }
        if svg.contains("<text") {
            violations.push(format!("{label}: the deterministic backend fell back to a host font:\n{svg}"));
        }

        let mut diagnostics = Vec::new();
        collect_layout_with_diagnostics(&root, 0.0, metrics.height, &mut diagnostics);
        if !diagnostics.is_empty() {
            violations.push(format!("{label}: the font could not outline every glyph: {diagnostics:?}"));
        }

        let positive = [
            ("width", metrics.width),
            ("height", metrics.height),
            ("depth", metrics.depth),
            ("baseline", metrics.baseline),
        ];
        for (name, value) in positive {
            if !value.is_finite() || value < 0.0 || (name != "depth" && value == 0.0) {
                violations.push(format!("{label}: {name} is not a positive metric, got {value}"));
            }
        }
    }

    assert!(violations.is_empty(), "{} gate failure(s):\n{}", violations.len(), violations.join("\n"));
}

/// Every painted point and rule corner falls inside a strictly positive viewBox,
/// so nothing the pipeline draws is clipped away.
#[test]
fn painted_ink_stays_inside_a_positive_view_box() {
    let corpus = load_corpus();
    let mut violations = Vec::new();

    for case in corpus.cases.iter().filter(|case| case.expected == "layout") {
        let (_, _, svg) = render(case);
        let label = format!("`{}` ({})", case.src, case.style);
        let (min_x, min_y, width, height) = view_box(&svg);
        let view_max_x = min_x + width;
        let view_max_y = min_y + height;

        if !(width.is_finite() && width > 0.0 && height.is_finite() && height > 0.0) {
            violations.push(format!("{label}: viewBox is {width} x {height}, not a positive size:\n{svg}"));
            continue;
        }

        let Some((ink_min_x, ink_min_y, ink_max_x, ink_max_y)) = ink_bounds(&svg)
        else {
            violations.push(format!("{label}: nothing was painted:\n{svg}"));
            continue;
        };

        let tolerance = 1e-3;
        if ink_min_x < min_x - tolerance
            || ink_min_y < min_y - tolerance
            || ink_max_x > view_max_x + tolerance
            || ink_max_y > view_max_y + tolerance
        {
            violations.push(format!(
                "{label}: ink {ink_min_x}..{ink_max_x} x {ink_min_y}..{ink_max_y} escapes viewBox {min_x}..{view_max_x} x {min_y}..{view_max_y}"
            ));
        }
    }

    assert!(violations.is_empty(), "{} clipped formula(s):\n{}", violations.len(), violations.join("\n"));
}

/// A display fraction sets its arguments a style larger and uses the display
/// shift constants, so its total height must clear 1.5x the text fraction.
#[test]
fn display_fraction_is_at_least_one_and_a_half_times_the_text_fraction() {
    let corpus = load_corpus();
    let display = corpus
        .cases
        .iter()
        .find(|case| {
            case.invariants.iter().any(|invariant| invariant == "display_fraction_height") && case.style == "display"
        })
        .expect("the corpus records a display fraction case");
    let text = corpus
        .cases
        .iter()
        .find(|case| case.group == display.group && case.style == "text")
        .expect("the corpus records the matching text fraction case");

    let display_total = total_extent(&display.src, true);
    let text_total = total_extent(&text.src, false);
    assert!(
        display_total >= text_total * 1.5,
        "`{}` display extent {display_total} must be at least 1.5x the text extent {text_total}",
        display.src
    );
}

/// A script fraction is set two styles down, so it stays shorter than the
/// text-style fraction of the same shape.
#[test]
fn script_fraction_is_shorter_than_the_text_fraction() {
    let corpus = load_corpus();
    let script = corpus
        .cases
        .iter()
        .find(|case| case.invariants.iter().any(|invariant| invariant == "script_fraction_shrinks"))
        .expect("the corpus records a script fraction case");
    let text = corpus
        .cases
        .iter()
        .find(|case| case.group == script.group && case.style == "text")
        .expect("the corpus records the matching text fraction case");

    let script_total = total_extent(&script.src, is_display(script));
    let text_total = total_extent(&text.src, false);
    assert!(
        script_total < text_total,
        "`{}` script extent {script_total} must stay under the text extent {text_total}",
        script.src
    );
}

/// A stretched delimiter is sized from the body it wraps, so the delimited
/// formula's extent must clear the bare body by a visible margin instead of
/// collapsing onto the one-em minimum.
#[test]
fn stretched_delimiters_cover_their_body() {
    const MIN_COVER: f64 = 1.05;

    let corpus = load_corpus();
    let mut violations = Vec::new();
    let mut checked = 0;

    for case in &corpus.cases {
        if !case.invariants.iter().any(|invariant| invariant == "delimiter_covers_body") {
            continue;
        }
        let body = case.body.as_deref().expect("`delimiter_covers_body` records the body");
        let display = is_display(case);
        let wrapped = total_extent(&case.src, display);
        let bare = total_extent(body, display);
        checked += 1;
        if wrapped < bare * MIN_COVER {
            violations.push(format!(
                "`{}` ({}) wraps `{body}` in {wrapped} but the body alone is {bare} (ratio {:.3} < {MIN_COVER})",
                case.src,
                case.style,
                wrapped / bare.max(f64::EPSILON),
            ));
        }
    }

    assert!(checked >= 3, "the corpus must exercise stretched delimiters, checked {checked}");
    assert!(violations.is_empty(), "{} delimiter(s) do not cover their body:\n{}", violations.len(), violations.join("\n"));
}

/// `\bar` is not a text mark: it paints a rule spanning the nucleus, so the
/// tree holds one rule and the SVG one rectangle.
#[test]
fn bar_accent_paints_a_rule_across_the_nucleus() {
    let corpus = load_corpus();
    let case = corpus
        .cases
        .iter()
        .find(|case| case.invariants.iter().any(|invariant| invariant == "rule_for_bar"))
        .expect("the corpus records a `\\bar` case");

    let (root, _, svg) = render(case);
    assert_eq!(rule_nodes(&root), 1, "`{}` must lower to exactly one rule: {root:#?}", case.src);
    assert_eq!(svg.matches("<rect ").count(), 1, "`{}` must paint exactly one rule rectangle:\n{svg}", case.src);
    assert!(!svg.contains("<text"), "`{}` must not fall back to a host font:\n{svg}", case.src);
}
