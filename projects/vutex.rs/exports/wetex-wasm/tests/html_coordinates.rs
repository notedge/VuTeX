//! Coordinate-space contract for the HTML backend.
//!
//! The fragment mirrors the paint tree's nested space: every group span's
//! `transform` carries only its own offset from the enclosing group, and every
//! leaf carries only its node-local coordinates. Composing the ancestor chain
//! must reproduce exactly the canvas geometry the SVG backend paints — no
//! offset may be counted twice, and no ink may fall outside the container the
//! metrics describe.

use serde_json::Value;
use wetex_wasm::{render_html_wasm, render_svg_wasm};

/// Syntax families shared with the TS corpus gate (`wetex/tests/syntax-corpus.test.ts`)
/// and the Rust corpus gate. Every entry must keep the HTML↔SVG geometry
/// invariants below: same leaf population, same composed anchors, same groups.
const CORPUS: &[&str] = &[
    // baseline shapes
    "\\frac{a}{b}",
    "\\sqrt{x}",
    "\\sum_{i=1}^{n}x_i",
    // scripts
    "x^{a_b}",
    "10^{10^{10}}",
    "e^{-x^2}",
    "x'",
    "f''(x)",
    // fractions
    "\\frac{\\frac{a}{b}}{c}",
    "\\dfrac{a}{b}",
    "\\tfrac{a}{b}",
    // radicals
    "\\sqrt[3]{x+y}",
    "\\sqrt{\\frac{a}{b}}",
    "\\sqrt{\\sqrt{x}}",
    // large operators
    "\\prod_{i=1}^{n} i",
    "\\oint_C f\\,dz",
    "\\lim_{x \\to 0} \\frac{\\sin x}{x}",
    // accents and over/under
    "\\vec{v}",
    "\\widehat{CD}",
    "\\overset{!}{=}",
    "\\underset{\\alpha}{\\rightarrow}",
    // delimiters
    "\\left\\lVert x \\right\\rVert",
    "\\left\\{ \\frac{a}{b} \\right\\}",
    "\\left\\langle v \\right\\rangle",
    "\\left[ \\begin{matrix} a & b \\\\ c & d \\end{matrix} \\right]",
    "\\left. \\frac{dy}{dx} \\right|_{0}",
    // environments
    "\\begin{smallmatrix} a & b \\\\ c & d \\end{smallmatrix}",
    "\\begin{gathered} a+b \\\\ c+d \\end{gathered}",
    "\\begin{split} a &= b \\\\ &= c \\end{split}",
    "\\begin{bmatrix}a&b\\\\c&d\\end{bmatrix}",
    "\\begin{cases}x&x>0\\\\-x&x\\le 0\\end{cases}",
    "\\begin{aligned}a&=b\\\\c&=d\\end{aligned}",
    "\\begin{alignedat}{2}x&=y&x&=z\\end{alignedat}",
    // spacing
    "a\\,b",
    "a\\;b",
    "a\\quad b",
    "a\\qquad b",
    // fonts
    "\\mathbf{Ax}",
    "\\mathbb{R}",
    "\\mathcal{L}",
    "\\mathrm{d}x",
    "\\mathsf{s}",
    "\\mathit{x}",
    // relations and dots
    "x \\le y \\ge z",
    "p \\Rightarrow q",
    "A \\leftarrow B",
    "a \\cdot b \\times c \\div d",
    "a_1 + \\cdots + a_n",
    // text
    "\\text{hello world}",
    "x + \\text{ text } + y",
    // nested structures
    "\\frac{\\sum_{i=1}^{n} i}{\\sqrt{n}}",
];

/// One positioned leaf with its canvas-composed box or anchor.
#[derive(Debug)]
struct Leaf {
    kind: &'static str,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

#[derive(Debug)]
struct Metrics {
    width: f64,
    height: f64,
    depth: f64,
}

fn parse(json: &str) -> Value {
    serde_json::from_str(json).expect("export must return valid JSON")
}

fn metrics_of(value: &Value) -> Metrics {
    Metrics {
        width: value["metrics"]["width"].as_f64().expect("width"),
        height: value["metrics"]["height"].as_f64().expect("height"),
        depth: value["metrics"]["depth"].as_f64().expect("depth"),
    }
}

/// Parse `key:` up to the following `px` inside a style attribute.
fn css_px(tag: &str, key: &str) -> f64 {
    let rest = tag.split_once(key).unwrap_or_else(|| panic!("missing `{key}` in: {tag}")).1;
    let raw = rest.split_once("px").unwrap_or_else(|| panic!("missing `px` after `{key}` in: {tag}")).0;
    raw.parse().unwrap_or_else(|_| panic!("bad number after `{key}`: {raw}"))
}

/// Parse a `translate(a, b)` pair, with or without `px` units.
fn translate_pair(tag: &str) -> (f64, f64) {
    let rest = tag.split_once("translate(").expect("missing translate(").1;
    let (a, b) = rest.split_once(',').expect("missing comma in translate()");
    let b = b.split_once(')').expect("missing ) in translate()").0;
    let a = a.trim().trim_end_matches("px");
    let b = b.trim().trim_end_matches("px");
    (a.parse().expect("bad translate x"), b.parse().expect("bad translate y"))
}

/// Split markup into tags, ignoring text content (which is escaped).
fn tags(markup: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = markup;
    while let Some(start) = rest.find('<') {
        let after = &rest[start..];
        match after.find('>') {
            Some(end) => {
                out.push(after[..=end].to_string());
                rest = &after[end + 1..];
            }
            None => break,
        }
    }
    out
}

/// The ordered group translates the HTML emits, pre-order.
fn html_groups(markup: &str) -> Vec<(f64, f64)> {
    tags(markup)
        .into_iter()
        .filter(|tag| tag.starts_with("<span") && tag.contains("transform:translate("))
        .map(|tag| translate_pair(&tag))
        .collect()
}

/// The ordered `<g>` translates the SVG backend emits, pre-order.
fn svg_groups(markup: &str) -> Vec<(f64, f64)> {
    tags(markup).into_iter().filter(|tag| tag.starts_with("<g ")).map(|tag| translate_pair(&tag)).collect()
}

/// Walk the HTML fragment, composing each leaf through its ancestor groups.
///
/// Returns the root group translate, the positioned leaves (anchors for text
/// and paths, boxes for rules) and the padded ink boxes of the svg fragments.
/// The container span sits at the canvas origin, so the first group span is
/// the paint tree's root.
fn html_leaves(markup: &str) -> ((f64, f64), Vec<Leaf>, Vec<Leaf>) {
    let mut stack: Vec<(f64, f64)> = Vec::new();
    let mut leaves: Vec<Leaf> = Vec::new();
    let mut ink: Vec<Leaf> = Vec::new();
    let mut root: Option<(f64, f64)> = None;
    let mut pending_svg: Option<(f64, f64, f64, f64, f64, f64)> = None;

    for tag in tags(markup) {
        if tag.starts_with("<span") {
            if tag.contains("transform:translate(") {
                let t = translate_pair(&tag);
                root.get_or_insert(t);
                stack.push(t);
            }
            else {
                // Container or leaf span: contributes no offset of its own.
                stack.push((0.0, 0.0));
            }
            // The container at the bottom of the stack is the canvas origin.
            let sum: (f64, f64) = stack.iter().skip(1).fold((0.0, 0.0), |a, b| (a.0 + b.0, a.1 + b.1));
            if tag.contains("background:currentColor") {
                leaves.push(Leaf {
                    kind: "rect",
                    x: sum.0 + css_px(&tag, "left:"),
                    y: sum.1 + css_px(&tag, "top:"),
                    w: css_px(&tag, "width:"),
                    h: css_px(&tag, "height:"),
                });
            }
            else if tag.contains("font-family:") {
                leaves.push(Leaf {
                    kind: "text",
                    x: sum.0 + css_px(&tag, "left:"),
                    y: sum.1 + css_px(&tag, "top:"),
                    w: 0.0,
                    h: 0.0,
                });
            }
        }
        else if tag.starts_with("</span") {
            stack.pop().expect("unbalanced </span>");
        }
        else if tag.starts_with("<svg") {
            let sum: (f64, f64) = stack.iter().skip(1).fold((0.0, 0.0), |a, b| (a.0 + b.0, a.1 + b.1));
            pending_svg = Some((
                sum.0,
                sum.1,
                css_px(&tag, "left:"),
                css_px(&tag, "top:"),
                css_px(&tag, "width:"),
                css_px(&tag, "height:"),
            ));
        }
        else if tag.starts_with("<path") {
            // The path anchor is group-local; the svg box only frames the ink.
            let (ax, ay) = translate_pair(&tag);
            let svg = pending_svg.take().expect("path outside an svg fragment");
            leaves.push(Leaf { kind: "path", x: svg.0 + ax, y: svg.1 + ay, w: 0.0, h: 0.0 });
            ink.push(Leaf { kind: "ink", x: svg.0 + svg.2, y: svg.1 + svg.3, w: svg.4, h: svg.5 });
        }
    }
    (root.expect("no root group"), leaves, ink)
}

/// Walk the SVG backend markup, composing each leaf through its `<g>` chain.
fn svg_leaves(markup: &str) -> Vec<Leaf> {
    let mut stack: Vec<(f64, f64)> = Vec::new();
    let mut leaves = Vec::new();
    for tag in tags(markup) {
        if tag.starts_with("<g ") {
            stack.push(translate_pair(&tag));
        }
        else if tag.starts_with("</g") {
            stack.pop().expect("unbalanced </g>");
        }
        else {
            let sum: (f64, f64) = stack.iter().fold((0.0, 0.0), |a, b| (a.0 + b.0, a.1 + b.1));
            if tag.starts_with("<rect") {
                let attr = |key: &str| -> f64 {
                    let rest = tag.split_once(key).expect("missing rect attr").1;
                    rest.split_once('"').expect("bad rect attr").0.parse().expect("bad rect number")
                };
                leaves.push(Leaf {
                    kind: "rect",
                    x: sum.0 + attr("x=\""),
                    y: sum.1 + attr("y=\""),
                    w: attr("width=\""),
                    h: attr("height=\""),
                });
            }
            else if tag.starts_with("<path") {
                let (ax, ay) = translate_pair(&tag);
                leaves.push(Leaf { kind: "path", x: sum.0 + ax, y: sum.1 + ay, w: 0.0, h: 0.0 });
            }
        }
    }
    leaves
}

fn sort_leaves(leaves: &mut [Leaf]) {
    leaves.sort_by(|a, b| a.kind.cmp(b.kind).then(a.x.total_cmp(&b.x)).then(a.y.total_cmp(&b.y)));
}

/// The composed geometry of the HTML fragment must equal the SVG backend's
/// within rounding, and every leaf box must sit inside the metrics container
/// up to a small ink tolerance. No font-dependent constant is asserted.
#[test]
fn html_geometry_matches_svg_and_stays_inside_the_container() {
    for source in CORPUS {
        let html = parse(&render_html_wasm(source, None));
        let svg = parse(&render_svg_wasm(source, None));
        let markup = html["html"].as_str().expect("html");
        let svg_markup = svg["svg"].as_str().expect("svg");
        assert!(!markup.is_empty(), "{source}: empty html");
        assert!(!svg_markup.is_empty(), "{source}: empty svg");
        let m = metrics_of(&html);

        let (root, mut leaves, ink) = html_leaves(markup);
        // The root group carries the baseline offset from the box top-left.
        assert_eq!(root.0, 0.0, "{source}: the root group must not shift horizontally");
        assert!((root.1 - m.height).abs() < 1e-3, "{source}: root translate {} != metrics height {}", root.1, m.height);

        // These strict renders paint outlines and rules only; a text run would
        // silently change the leaf population the comparison below expects.
        assert!(!leaves.iter().any(|l| l.kind == "text"), "{source}: unexpected text run in html");

        let mut svg_leaves = svg_leaves(svg_markup);
        sort_leaves(&mut leaves);
        sort_leaves(&mut svg_leaves);
        assert_eq!(leaves.len(), svg_leaves.len(), "{source}: leaf count differs between backends");
        for (h, s) in leaves.iter().zip(&svg_leaves) {
            assert_eq!(h.kind, s.kind, "{source}: leaf kind order differs");
            let tol = 2e-3;
            assert!((h.x - s.x).abs() < tol && (h.y - s.y).abs() < tol, "{source}: {h:?} html anchor vs {s:?} svg anchor");
            if h.kind == "rect" {
                assert!((h.w - s.w).abs() < tol && (h.h - s.h).abs() < tol, "{source}: {h:?} html rect vs {s:?} svg rect");
            }
        }

        // Boundedness: composed boxes stay inside the container the metrics
        // describe, up to the 1px svg pad plus a small glyph-overshoot slack.
        let slack = 2.0;
        for leaf in leaves.iter().chain(ink.iter()) {
            assert!(
                leaf.x >= -slack
                    && leaf.x + leaf.w <= m.width + slack
                    && leaf.y >= -slack
                    && leaf.y + leaf.h <= m.height + m.depth + slack,
                "{source}: leaf {leaf:?} outside container {}x{}",
                m.width,
                m.height + m.depth
            );
        }
    }
}

/// Group spans carry only their own increment: the pre-order sequence of
/// HTML group translates must equal the SVG backend's `<g>` translate
/// sequence, one per paint-tree group. The double-offset shape accumulated
/// ancestor offsets into each group transform, which breaks this equality at
/// the first nested group.
#[test]
fn group_transforms_are_relative_increments() {
    for source in CORPUS {
        let markup = parse(&render_html_wasm(source, None))["html"].as_str().expect("html").to_string();
        let svg_markup = parse(&render_svg_wasm(source, None))["svg"].as_str().expect("svg").to_string();
        let html = html_groups(&markup);
        let svg = svg_groups(&svg_markup);
        assert!(!html.is_empty(), "{source}: html emitted no groups");
        assert_eq!(html.len(), svg.len(), "{source}: group count differs between backends");
        for (i, (h, s)) in html.iter().zip(&svg).enumerate() {
            assert!(
                (h.0 - s.0).abs() < 2e-3 && (h.1 - s.1).abs() < 2e-3,
                "{source}: group {i}: html translate {h:?} != svg translate {s:?}"
            );
        }
    }
}

/// Dump the rendered fragments to the system temp directory so the produced
/// markup can be inspected outside the test run. The assertions above are the
/// gate; this file is the human-visible artifact.
#[test]
fn writes_an_inspectable_artifact_to_the_temp_dir() {
    let mut page =
        String::from("<!doctype html><meta charset=\"utf-8\"><title>wetex html coords</title><body style=\"font-size:16px\">");
    for source in CORPUS {
        let html = parse(&render_html_wasm(source, None));
        page.push_str(&format!("<p>{} — {}</p>\n", source, html["html"].as_str().expect("html")));
    }
    page.push_str("</body>");
    let path = std::env::temp_dir().join("vutex_html_coords_probe.html");
    std::fs::write(&path, page).expect("write artifact");
    assert!(path.exists(), "artifact missing at {}", path.display());
}
