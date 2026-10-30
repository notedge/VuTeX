//! PDF encoding of a laid-out formula.
//!
//! A math page must carry the outlines the layout tree already holds: no host
//! text operator, one path paint per glyph outline and per rule, and a glyph the
//! embedded font cannot outline is reported instead of being drawn with a host
//! serif face.

use vutex_layout::{
    math::{Glyph, LayoutNode, collect_layout, collect_layout_with_diagnostics, layout_formula},
    plan::{LayoutItem, LayoutPage, LayoutPlan},
};
use vutex_presentation::build_display_presentation;
use vutex_render_pdf::render_pdf_with_diagnostics;
use vutex_types::{
    Diagnostic,
    math::{Fixed, FontStyle, GlyphMetrics},
};

/// Page geometry every formula below is placed on.
const PAGE_WIDTH: f64 = 400.0;
const PAGE_HEIGHT: f64 = 200.0;
/// Baseline the formula is anchored on.
const BASELINE: f64 = 120.0;

/// Formulas the acceptance list covers, in display style.
fn formulas() -> &'static [&'static str] {
    &[
        "x+y",
        "\\frac{a}{b}",
        "\\sqrt{x}",
        "\\sqrt[3]{x}",
        "\\left(a\\right)",
        "\\left(\\frac{a}{b}\\right)",
        "\\left\\{ \\begin{matrix} a \\\\ b \\\\ c \\end{matrix} \\right.",
        "|x|",
        "\\sum_{i=1}^{n} x_i",
        "\\begin{pmatrix} a & b \\\\ c & d \\end{pmatrix}",
    ]
}

/// Number of vector primitives a layout tree paints: every outlined glyph, every
/// rule, and every standalone path.
fn path_primitives(node: &LayoutNode) -> usize {
    match node {
        LayoutNode::Glyph(glyph) => usize::from(glyph.path_data.is_some()),
        LayoutNode::Rule { .. } | LayoutNode::Path(_) => 1,
        LayoutNode::HBox(hbox) => hbox.children.iter().map(path_primitives).sum(),
        LayoutNode::VBox(vbox) => vbox.children.iter().map(path_primitives).sum(),
        _ => 0,
    }
}

/// Number of `LayoutNode::Rule` boxes a layout tree paints.
fn rule_primitives(node: &LayoutNode) -> usize {
    match node {
        LayoutNode::Rule { .. } => 1,
        LayoutNode::HBox(hbox) => hbox.children.iter().map(rule_primitives).sum(),
        LayoutNode::VBox(vbox) => vbox.children.iter().map(rule_primitives).sum(),
        _ => 0,
    }
}

/// Number of painted blocks that are curve-free closed four-point rectangles
/// spanning at least `min_width` points.
///
/// A math rule reaches PDF as such a path because `LayoutItem::Rule` carries no
/// depth: the collector emits the bar as a closed box spanning `height + depth`
/// instead of dropping the part below the baseline. Glyph outlines of bar-like
/// characters are narrower than a rule, so the width filter keeps them apart.
fn rectangle_fills(content: &str, min_width: f64) -> usize {
    content
        .split("q\n0.0 f\n")
        .skip(1)
        .filter(|block| {
            let body = block.split("\nf\n").next().unwrap_or_default();
            let lines: Vec<&str> = body.lines().collect();
            let ops: Vec<&str> = lines.iter().filter_map(|line| line.split_whitespace().last()).collect();
            if ops != ["m", "l", "l", "l", "h"] {
                return false;
            }
            let xs: Vec<f64> =
                lines.iter().take(4).filter_map(|line| line.split_whitespace().next()?.parse::<f64>().ok()).collect();
            xs.len() == 4 && (xs[0] - xs[2]).abs() >= min_width
        })
        .count()
}

/// Lay out one formula, lower it to display primitives, and render a PDF page.
fn render_formula(source: &str) -> (Vec<u8>, Vec<LayoutItem>, Vec<Diagnostic>) {
    let (root, _layout) = layout_formula(source, true).unwrap_or_else(|e| panic!("layout {source}: {e}"));
    let mut diagnostics = Vec::new();
    let items = collect_layout_with_diagnostics(&root, 72.0, BASELINE, &mut diagnostics);
    let plan = LayoutPlan { pages: vec![LayoutPage { width: PAGE_WIDTH, height: PAGE_HEIGHT, items: items.clone() }] };
    let display = build_display_presentation(&plan);
    let outcome = render_pdf_with_diagnostics(&display).expect("pdf render");
    (outcome.pdf, items, diagnostics)
}

/// Joined body of every page content stream in `bytes`.
fn page_content(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes).into_owned();
    let mut contents: Vec<String> = Vec::new();
    let mut cursor = 0;
    while let Some(found) = text[cursor..].find("/Contents ") {
        let at = cursor + found + "/Contents ".len();
        let digits: String = text[at..].chars().take_while(char::is_ascii_digit).collect();
        let number: usize = digits.parse().expect("content object number");
        let marker = format!("\n{number} 0 obj\n");
        let object = text.find(&marker).expect("content object");
        let body_start = object + marker.len();
        let body_end = body_start + text[body_start..].find("\nendobj").expect("object end");
        let body = &text[body_start..body_end];
        let stream_start = body_start + body.find("stream\n").expect("stream") + "stream\n".len();
        let stream_end = body_start + body.rfind("\nendstream").expect("stream end");
        contents.push(text[stream_start..stream_end].to_string());
        cursor = body_end;
    }
    assert!(!contents.is_empty(), "the page carries a content stream");
    contents.join("\n")
}

/// First `m` operand pair of every painted path, in paint order.
fn path_origins(content: &str) -> Vec<(f64, f64)> {
    let mut origins = Vec::new();
    for block in content.split("q\n").skip(1) {
        if let Some(line) = block.lines().find(|line| line.ends_with(" m")) {
            let numbers: Vec<f64> =
                line.trim_end_matches(" m").split_whitespace().map(|value| value.parse().expect("path operand")).collect();
            origins.push((numbers[0], numbers[1]));
        }
    }
    origins
}

#[test]
fn math_pages_paint_outlines_without_host_text() {
    for source in formulas() {
        let (pdf, items, diagnostics) = render_formula(source);
        let root_pdf = String::from_utf8_lossy(&pdf).into_owned();
        let content = page_content(&pdf);
        let (root, _) = layout_formula(source, true).expect("layout");

        assert!(diagnostics.is_empty(), "{source}: the embedded font must outline every glyph: {diagnostics:?}");
        assert!(
            items.iter().all(|item| !matches!(item, LayoutItem::TextRun(_))),
            "{source}: a math glyph never lowers to a text run: {items:#?}"
        );
        assert!(!content.contains("BT\n"), "{source}: a math page opens no text object: {content}");
        assert!(!content.contains(" Tj\n") && !content.contains(" TJ\n"), "{source}: no host text operator: {content}");
        assert!(!root_pdf.contains("/BaseFont"), "{source}: no host font is referenced: {root_pdf}");

        let expected = path_primitives(&root);
        assert!(expected > 0, "{source}: the layout tree must paint something");
        assert_eq!(
            content.matches("\nf\n").count(),
            expected,
            "{source}: one fill per glyph outline and rule, expected {expected}: {content}"
        );
        let rules = rule_primitives(&root);
        let painted = content.matches(" re\n").count() + rectangle_fills(&content, 2.0);
        assert_eq!(painted, rules, "{source}: every rule paints as a filled rectangle, expected {rules}: {content}");
        assert!(!content.contains("\nS\n"), "{source}: a math page strokes nothing: {content}");
    }
}

#[test]
fn superscript_and_subscript_shift_their_baselines() {
    let (superscript, _, _) = render_formula("x^2");
    let (subscript, _, _) = render_formula("x_2");

    let superscript = path_origins(&page_content(&superscript));
    let subscript = path_origins(&page_content(&subscript));
    assert_eq!(superscript.len(), 2, "x^2 paints the base and the script: {superscript:?}");
    assert_eq!(subscript.len(), 2, "x_2 paints the base and the script: {subscript:?}");

    // PDF grows y upward from the page bottom, so a raised script has the larger
    // coordinate. The HBox `shift` must move the script up, never down.
    let (base, script) = (superscript[0].1, superscript[1].1);
    assert!(script > base + 1.0, "superscript baseline {script} must sit above the base baseline {base}");
    let (base, script) = (subscript[0].1, subscript[1].1);
    assert!(script < base - 1.0, "subscript baseline {script} must sit below the base baseline {base}");
}

#[test]
fn glyph_without_an_outline_is_reported_instead_of_falling_back() {
    let glyph = LayoutNode::Glyph(Glyph {
        char: Some('中'),
        font_family: "default".to_string(),
        font_style: FontStyle::Normal,
        size: Fixed::from_f64(16.0),
        metrics: GlyphMetrics {
            width: Fixed::from_f64(10.0),
            height: Fixed::from_f64(10.0),
            depth: Fixed::ZERO,
            italic_correction: Fixed::ZERO,
            top_accent_attachment: Fixed::ZERO,
        },
        path_data: None,
        font_id: None,
        glyph_id: None,
    });

    let mut diagnostics = Vec::new();
    let items = collect_layout_with_diagnostics(&glyph, 72.0, BASELINE, &mut diagnostics);
    assert!(items.is_empty(), "an outline-less glyph paints nothing: {items:#?}");
    assert_eq!(diagnostics.len(), 1, "the gap is structured, not silent: {diagnostics:?}");
    assert_eq!(diagnostics[0].code, "vutex.layout.math.outline_missing");
    assert!(diagnostics[0].message.contains("U+4E2D"), "the report names the code point: {diagnostics:?}");

    // The compatibility wrapper keeps the same policy instead of resurrecting a
    // serif run for callers that collect without a diagnostic sink.
    let plain = collect_layout(&glyph, 72.0, BASELINE);
    assert!(plain.is_empty(), "the plain collector paints nothing either: {plain:#?}");

    // A real formula whose glyph the math font cannot outline follows the same
    // rule: the PDF carries no text operator for it.
    let (pdf, items, diagnostics) = render_formula("\\text{中}");
    assert!(!diagnostics.is_empty(), "the CJK glyph is reported: {diagnostics:?}");
    assert!(items.iter().all(|item| !matches!(item, LayoutItem::TextRun(_))), "{items:#?}");
    let content = page_content(&pdf);
    assert!(!content.contains(" Tj\n") && !content.contains("/BaseFont"), "no host glyph is borrowed: {content}");
}
