//! Document layout integration tests for display math blocks.

use notedown_ir::{Block, DocumentGraph, DocumentId, Inline};
use vutex_layout::{
    Diagrams, LayoutHint,
    document::layout_with_diagnostics,
    layout,
    math::layout_formula_with_result,
    plan::{LayoutItem, LayoutPage, Path, Rule, TextRun},
};

fn math_graph(content: &str) -> DocumentGraph {
    let mut graph = DocumentGraph::new(DocumentId(7));
    graph.push_block(Block::Math { content: content.to_string(), language: Some("tex".to_string()) });
    graph
}

fn text_runs(page: &LayoutPage) -> Vec<&TextRun> {
    page.items
        .iter()
        .filter_map(|item| match item {
            LayoutItem::TextRun(run) => Some(run),
            _ => None,
        })
        .collect()
}

fn rules(page: &LayoutPage) -> Vec<&Rule> {
    page.items
        .iter()
        .filter_map(|item| match item {
            LayoutItem::Rule(rule) => Some(rule),
            _ => None,
        })
        .collect()
}

fn paths(page: &LayoutPage) -> Vec<&Path> {
    page.items
        .iter()
        .filter_map(|item| match item {
            LayoutItem::Path(p) => Some(p),
            _ => None,
        })
        .collect()
}

#[test]
fn simple_formula_emits_left_to_right_glyphs_on_one_baseline() {
    let plan = layout(&math_graph("x + y"));
    let hint = LayoutHint::a4();
    assert_eq!(plan.pages.len(), 1);
    let page = &plan.pages[0];
    let runs = text_runs(page);
    let paths = paths(page);

    // Glyphs are emitted as either text runs or paths depending on whether the
    // embedded font supplied an outline. Either way we expect three glyphs.
    assert!(runs.len() + paths.len() >= 3, "expected three visible glyphs as runs or paths: runs={runs:#?} paths={paths:#?}");

    // Ordering and baseline checks: collect all non-rule glyph items.
    let mut glyph_items: Vec<(f64, f64, f64)> = page
        .items
        .iter()
        .filter_map(|item| match item {
            LayoutItem::TextRun(r) => Some((r.x, r.y, 0.0)),
            LayoutItem::Path(p) if !p.d.is_empty() => Some((p.x, p.y, 0.0)),
            _ => None,
        })
        .collect();
    glyph_items.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());

    for window in glyph_items.windows(2) {
        assert!(window[1].0 > window[0].0, "glyphs must advance left to right: {glyph_items:#?}");
        assert!((window[1].1 - window[0].1).abs() < 1e-9, "glyphs share a baseline: {glyph_items:#?}");
    }

    if let Some(first) = glyph_items.first() {
        assert!(first.0 >= hint.margin.left - 1e-6, "first glyph at {} starts before the content box", first.0);
    }
}

#[test]
fn display_formula_is_centred_in_the_content_column() {
    let plan = layout(&math_graph("x + y"));
    let hint = LayoutHint::a4();
    let page = &plan.pages[0];
    let (formula, _, _) = layout_formula_with_result("x + y", true, hint.base_font_size).expect("formula layout");

    let first = page
        .items
        .iter()
        .filter_map(|item| match item {
            LayoutItem::TextRun(r) if matches!(r.text.as_str(), "x" | "+" | "y") => Some(r.x),
            LayoutItem::Path(p) if !p.d.is_empty() => Some(p.x),
            _ => None,
        })
        .next()
        .expect("at least one glyph must be placed");

    assert!(first > hint.margin.left, "centred formula {} must start inside the margin, not on it", first);

    let formula_width = formula.metrics.width.to_f64();
    let expected_offset = (hint.content_width() - formula_width) / 2.0;
    let actual_offset = first - hint.margin.left;
    assert!(
        (actual_offset - expected_offset).abs() < 1.0,
        "left offset {actual_offset} should centre a {} pt formula in {} pt",
        formula_width,
        hint.content_width()
    );
}

#[test]
fn fraction_emits_a_rule_between_numerator_and_denominator() {
    let plan = layout(&math_graph("\\frac{a}{b}"));
    let page = &plan.pages[0];
    let bars = rules(page);

    assert_eq!(bars.len(), 1, "a fraction contributes exactly one bar: {bars:#?}");
    let bar = bars[0];
    assert!(bar.x2 > bar.x1, "fraction bar must have width: {bar:?}");
    assert!(bar.thickness >= 0.4, "fraction bar must stay visible: {bar:?}");

    // Numerator and denominator may be emitted as paths or text runs. Look up
    // both shapes so the test is robust to whichever branch fires.
    let mut glyph_positions: Vec<(f64, f64)> = page
        .items
        .iter()
        .filter_map(|item| match item {
            LayoutItem::TextRun(r) if matches!(r.text.as_str(), "a" | "b") => Some((r.x, r.y)),
            LayoutItem::Path(p) if !p.d.is_empty() => Some((p.x, p.y)),
            _ => None,
        })
        .collect();
    glyph_positions.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());

    assert!(glyph_positions.len() >= 2, "numerator and denominator must be present");
    let numerator = glyph_positions[0];
    let denominator = glyph_positions[glyph_positions.len() - 1];
    assert!(numerator.1 < bar.y1, "numerator baseline {} must sit above the bar at {}", numerator.1, bar.y1);
    assert!(denominator.1 > bar.y1, "denominator baseline {} must sit below the bar at {}", denominator.1, bar.y1);
}

#[test]
fn unparseable_formula_falls_back_to_its_raw_source() {
    let source = "\\notacommand{x}";
    let plan = layout(&math_graph(source));
    let page = &plan.pages[0];
    let runs = text_runs(page);

    assert!(!runs.is_empty(), "fallback must still paint something");
    let painted: String = runs.iter().map(|run| run.text.as_str()).collect();
    assert_eq!(painted, source, "fallback should render the raw source verbatim: {runs:#?}");
    assert!(rules(page).is_empty(), "text fallback emits no rules");
    assert!(runs.iter().all(|run| run.y <= page.height));
}

#[test]
fn foreign_math_language_is_not_parsed_as_tex() {
    let mut graph = DocumentGraph::new(DocumentId(13));
    graph.push_block(Block::Math { content: "\\frac{a}{b}".to_string(), language: Some("typst".to_string()) });
    let (plan, diagnostics) = layout_with_diagnostics(&graph, &LayoutHint::a4(), &Diagrams::default());

    assert!(
        diagnostics.iter().any(|item| item.code == "vutex.layout.math.unsupported_language"),
        "a non-TeX math language must report its missing lowering: {diagnostics:#?}"
    );
    let page = &plan.pages[0];
    assert!(rules(page).is_empty(), "foreign math must not run through the TeX fraction layout");
    let painted: String = text_runs(page).iter().map(|run| run.text.as_str()).collect();
    assert_eq!(painted, "\\frac{a}{b}", "the source must stay visible as text");
}

#[test]
fn math_blocks_paginate_without_leaving_the_page() {
    let mut graph = DocumentGraph::new(DocumentId(8));
    for _ in 0..80 {
        graph.push_block(Block::Math { content: "\\frac{a}{b}".to_string(), language: None });
    }

    let plan = layout(&graph);
    let hint = LayoutHint::a4();
    assert!(plan.pages.len() > 1, "many math blocks must paginate, got {} page(s)", plan.pages.len());

    for page in &plan.pages {
        assert!(!page.items.is_empty(), "pages are only created when content moves to them");
        for item in &page.items {
            match item {
                LayoutItem::TextRun(run) => {
                    assert!(run.y >= hint.margin.top - 1e-9, "baseline {} above the top margin", run.y);
                    assert!(run.y <= page.height, "baseline {} outside page height {}", run.y, page.height);
                    assert!(run.x <= page.width, "x {} outside page width {}", run.x, page.width);
                }
                LayoutItem::Rule(rule) => {
                    assert!(rule.y1 >= 0.0 && rule.y1 <= page.height, "rule {} outside page height {}", rule.y1, page.height);
                    assert!(rule.x1 >= 0.0 && rule.x2 <= page.width, "rule spans {}..{}", rule.x1, rule.x2);
                }
                LayoutItem::Path(p) => {
                    assert!(p.y >= hint.margin.top - 1e-9, "path baseline {} above the top margin", p.y);
                    assert!(p.y <= page.height + 1e-9, "path baseline {} outside page height {}", p.y, page.height);
                }
                _ => {}
            }
        }
    }

    // Every math block contributes a fraction bar, none was dropped.
    let bars: usize = plan.pages.iter().map(|page| rules(page).len()).sum();
    assert_eq!(bars, 80, "expected one fraction bar per block, got {bars}");
}

/// Text baseline height as a fraction of the font size, mirroring layout.
const ASCENT: f64 = 0.8;

/// One-line paragraph followed by a display formula and another paragraph.
fn paragraph_display_graph(content: &str) -> DocumentGraph {
    let mut graph = DocumentGraph::new(DocumentId(9));
    graph.push_block(Block::Paragraph { content: vec![Inline::Text { text: "Lead in.".to_string() }] });
    graph.push_block(Block::Math { content: content.to_string(), language: None });
    graph.push_block(Block::Paragraph { content: vec![Inline::Text { text: "Tail.".to_string() }] });
    graph
}

/// Vertical ink extent `(top, bottom)` of the formula primitives on a page.
fn formula_ink(page: &LayoutPage) -> (f64, f64) {
    let mut top = f64::INFINITY;
    let mut bottom = f64::NEG_INFINITY;
    for item in &page.items {
        match item {
            LayoutItem::Path(p) => {
                top = top.min(p.y - p.height);
                bottom = bottom.max(p.y + p.depth);
            }
            LayoutItem::Rule(rule) => {
                top = top.min(rule.y1 - rule.thickness / 2.0);
                bottom = bottom.max(rule.y2 + rule.thickness / 2.0);
            }
            _ => {}
        }
    }
    assert!(top.is_finite() && bottom.is_finite(), "the formula must paint primitives: {page:#?}");
    (top, bottom)
}

/// Space above and below a display formula, measured from the surrounding
/// paragraph line boxes to the formula's painted ink.
fn display_gaps(page: &LayoutPage, hint: &LayoutHint) -> (f64, f64) {
    let runs = text_runs(page);
    assert_eq!(runs.len(), 2, "one paragraph line above and below the display: {runs:#?}");
    let size = hint.base_font_size;
    let (ink_top, ink_bottom) = formula_ink(page);
    let above = ink_top - (runs[0].y + (hint.line_height - ASCENT) * size);
    let below = (runs[1].y - ASCENT * size) - ink_bottom;
    (above, below)
}

#[test]
fn display_math_uses_tex_display_skips_above_and_below() {
    let hint = LayoutHint::a4();
    // A formula wider than the column is centred at the left margin, so the
    // short line above it cannot make the display short.
    let wide = vec!["\\frac{a}{b}"; 24].join("+");
    let (formula, _, _) = layout_formula_with_result(&wide, true, hint.base_font_size).expect("formula layout");
    assert!(
        formula.metrics.width.to_f64() > hint.content_width(),
        "the display must span the column, got {}",
        formula.metrics.width.to_f64()
    );

    let plan = layout(&paragraph_display_graph(&wide));
    assert_eq!(plan.pages.len(), 1, "the sample fits one page");
    let (above, below) = display_gaps(&plan.pages[0], &hint);

    // TeX's `\abovedisplayskip` / `\belowdisplayskip` are 10 pt plus 2 minus 5
    // at a 10 pt body, scaled to the 11 pt body here.
    let skip = hint.base_font_size;
    assert!(
        (above - skip).abs() <= 2.0,
        "display above skip {above} pt must follow `\\abovedisplayskip` ({skip} pt +2 -5): {:?}",
        text_runs(&plan.pages[0])
    );
    assert!(
        (below - skip).abs() <= 2.0,
        "display below skip {below} pt must follow `\\belowdisplayskip` ({skip} pt +2 -5): {:?}",
        text_runs(&plan.pages[0])
    );
}

#[test]
fn display_math_after_a_short_line_uses_the_short_skips() {
    let hint = LayoutHint::a4();
    let plan = layout(&paragraph_display_graph("x+y"));
    assert_eq!(plan.pages.len(), 1, "the sample fits one page");
    let (above, below) = display_gaps(&plan.pages[0], &hint);

    // `\abovedisplayshortskip` is 0 pt and `\belowdisplayshortskip` is 6 pt
    // plus 3 minus 3, both scaled to the body size.
    assert!(
        above.abs() < 1.0,
        "short display above skip {above} pt must collapse to `\\abovedisplayshortskip`: {:?}",
        text_runs(&plan.pages[0])
    );
    let short_below = 0.6 * hint.base_font_size;
    assert!(
        (below - short_below).abs() <= 1.0,
        "short display below skip {below} pt must follow `\\belowdisplayshortskip` ({short_below} pt): {:?}",
        text_runs(&plan.pages[0])
    );
    // The short form must be tighter than the full display skips in both
    // directions, which is the whole point of the TeX distinction.
    assert!(above < hint.base_font_size - 2.0, "short above skip {above} pt is not tighter than the full skip");
    assert!(below < hint.base_font_size - 2.0, "short below skip {below} pt is not tighter than the full skip");
}

#[test]
fn spacing_commands_never_raise_outline_warnings_in_a_document() {
    let hint = LayoutHint::a4();
    // `\,`, `\:` and `\ ` reach layout as text spaces. They advance the pen
    // without ink, so the report must not carry an outline gap for them.
    for source in ["a\\ b", "a\\quad b", "\\text{a b}"] {
        let mut graph = DocumentGraph::new(DocumentId(11));
        graph.push_block(Block::Math { content: source.to_string(), language: None });
        graph.push_block(Block::Paragraph {
            content: vec![Inline::InlineMath { content: source.to_string(), language: Some("tex".to_string()) }],
        });

        let (plan, diagnostics) = layout_with_diagnostics(&graph, &hint, &Diagrams::default());
        assert_eq!(plan.pages.len(), 1, "{source} fits one page");
        let gaps: Vec<_> = diagnostics.iter().filter(|d| d.code == "vutex.layout.math.outline_missing").collect();
        assert!(gaps.is_empty(), "{source}: whitespace must not be reported as a missing outline: {gaps:#?}");
    }
}
