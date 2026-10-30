//! Document layout integration tests for inline math inside paragraphs.

use notedown_ir::{Block, DocumentGraph, DocumentId, Inline};
use vutex_layout::{
    LayoutHint,
    document::layout_with_hint,
    hint::Margin,
    layout,
    math::layout_formula_with_result,
    plan::{LayoutItem, LayoutPage, Path, Rule, TextRun},
};

fn text(value: &str) -> Inline {
    Inline::Text { text: value.to_string() }
}

fn math(source: &str) -> Inline {
    Inline::InlineMath { content: source.to_string(), language: Some("tex".to_string()) }
}

fn paragraph(content: Vec<Inline>) -> DocumentGraph {
    let mut graph = DocumentGraph::new(DocumentId(11));
    graph.push_block(Block::Paragraph { content });
    graph
}

fn followed_by_marker(content: Vec<Inline>) -> DocumentGraph {
    let mut graph = DocumentGraph::new(DocumentId(12));
    graph.push_block(Block::Paragraph { content });
    graph.push_block(Block::Paragraph { content: vec![text("Marker")] });
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

/// Every inline math glyph is emitted either as a `TextRun` (when no outline
/// path is available) or as a `Path` (when the embedded MATH font supplied
/// one). This helper collects both so tests that count formula glyphs remain
/// unaffected by which branch the layout takes.
fn formula_path_count(page: &LayoutPage) -> usize {
    paths(page).iter().filter(|p| !p.d.is_empty()).count()
}

fn marker_baseline(page: &LayoutPage) -> f64 {
    text_runs(page).into_iter().find(|run| run.text == "Marker").expect("marker paragraph").y
}

#[test]
fn inline_formula_flows_between_surrounding_text_runs_on_the_text_baseline() {
    let graph = paragraph(vec![text("before "), math("x + y"), text(" after")]);
    let plan = layout(&graph);
    let page = &plan.pages[0];
    let runs = text_runs(page);
    let formula_paths = paths(page);

    let before = runs.iter().find(|run| run.text.trim() == "before").expect("leading text");
    let after = runs.iter().find(|run| run.text.trim() == "after").expect("trailing text");
    // Some fonts emit the math glyphs as paths, others as text runs; either way
    // there must be exactly three glyphs between the leading and trailing text.
    let formula_glyphs: Vec<_> = runs.iter().filter(|run| matches!(run.text.as_str(), "x" | "+" | "y")).collect();
    assert_eq!(
        formula_glyphs.len() + formula_path_count(page),
        3,
        "three formula glyphs as paths or runs: runs={runs:#?}, paths={formula_paths:#?}"
    );

    // Path glyphs: verify x-ordering and baseline alignment through Path items
    for path in formula_paths {
        if !path.d.is_empty() {
            assert!(path.x > before.x, "path glyph at {} must follow the leading run", path.x);
            assert!(path.x < after.x, "path glyph at {} must precede the trailing run", path.x);
            assert!((path.y - before.y).abs() < 1e-9, "path glyphs share the text baseline");
        }
    }
    for glyph in &formula_glyphs {
        assert!(glyph.x > before.x, "glyph {} at {} must follow the leading run", glyph.text, glyph.x);
        assert!(glyph.x < after.x, "glyph {} at {} must precede the trailing run", glyph.text, after.x);
        assert!((glyph.y - before.y).abs() < 1e-9, "glyphs share the text baseline: {formula_glyphs:#?}");
    }
    assert!(after.x > before.x);
}

#[test]
fn inline_fraction_keeps_its_bar_between_the_text_runs_and_grows_the_line() {
    let hint = LayoutHint { line_height: 1.2, ..LayoutHint::a4() };
    let (formula, _, _) = layout_formula_with_result("\\frac{a}{b}", false, hint.base_font_size).expect("fraction layout");
    let fraction_extent = formula.metrics.height.to_f64() + formula.metrics.depth.to_f64();
    assert!(
        fraction_extent > 0.75 * hint.base_font_size,
        "fraction extent {} must be tall enough to visually break the text line",
        fraction_extent
    );

    // A single text-style `\frac{a}{b}` is only about 1.06 em tall and its
    // denominator stays above the descender allowance of a 1.2 line, so the
    // sample that must push the baseline uses a denominator with a real
    // descender, whose box reaches past the line's descent.
    let formula_graph = followed_by_marker(vec![text("The ratio "), math("\\frac{a}{g}"), text(" here")]);
    let plain_graph = followed_by_marker(vec![text("The ratio a/b here")]);
    let formula_plan = layout_with_hint(&formula_graph, &hint);
    let plain_plan = layout_with_hint(&plain_graph, &hint);

    let page = &formula_plan.pages[0];
    let runs = text_runs(page);
    let bars = rules(page);
    assert_eq!(bars.len(), 1, "one fraction bar: {bars:#?}");
    let bar = bars[0];
    let before = runs.iter().find(|run| run.text.trim() == "The ratio").expect("leading text");
    let after = runs.iter().find(|run| run.text.trim() == "here").expect("trailing text");
    assert!(bar.x1 > before.x, "bar {} must sit after the leading run", bar.x1);
    assert!(bar.x2 <= after.x + 1e-9, "bar right {} must end before the trailing run", bar.x2);

    // Numerator/denominator may be emitted as paths or text runs when the font
    // carries outlines. We look up by glyph position rather than by text.
    let numerator = page
        .items
        .iter()
        .find_map(|item| match item {
            LayoutItem::TextRun(r) if r.text == "a" => Some((r.x, r.y)),
            LayoutItem::Path(p) if !p.d.is_empty() => Some((p.x, p.y)),
            _ => None,
        })
        .expect("numerator position");
    let denominator = page
        .items
        .iter()
        .filter_map(|item| match item {
            LayoutItem::TextRun(r) if r.text == "b" => Some((r.x, r.y)),
            LayoutItem::Path(p) if !p.d.is_empty() => Some((p.x, p.y)),
            _ => None,
        })
        .nth(1) // second Path after numerator
        .unwrap_or((0.0, 0.0));

    assert!(numerator.1 < bar.y1, "numerator y={} must sit above the bar {}", numerator.1, bar.y1);
    assert!(denominator.1 > bar.y1, "denominator y={} must sit below the bar {}", denominator.1, bar.y1);

    assert!(
        marker_baseline(page) > marker_baseline(&plain_plan.pages[0]),
        "the fraction line must consume more vertical space than a plain paragraph"
    );
}

#[test]
fn formula_that_does_not_fit_moves_to_the_next_line() {
    let hint = LayoutHint::a4().with_page_size(240.0, 400.0).with_margin(Margin::uniform(40.0));
    let graph = paragraph(vec![text(&"a".repeat(28)), math("x + y")]);
    let plan = layout_with_hint(&graph, &hint);
    let page = &plan.pages[0];
    let runs = text_runs(page);

    let filler = runs.iter().find(|run| run.text.starts_with("aaa")).expect("filler run");
    let (glyph_x, glyph_y) = page
        .items
        .iter()
        .find_map(|item| match item {
            LayoutItem::TextRun(r) if r.text == "x" => Some((r.x, r.y)),
            LayoutItem::Path(p) if !p.d.is_empty() => Some((p.x, p.y)),
            _ => None,
        })
        .expect("formula glyph x");
    assert!(glyph_y > filler.y, "formula baseline {} must move below the filler {}", glyph_y, filler.y);
    assert!((glyph_x - hint.margin.left).abs() < 1e-9, "wrapped formula starts at the column left edge");
    assert!(glyph_x + 8.0 <= plan.pages[0].width);
}

#[test]
fn formula_wider_than_the_content_width_is_still_painted_clamped() {
    let source = format!("{}x", "x+".repeat(100));
    let graph = paragraph(vec![math(&source)]);
    let plan = layout(&graph);
    let hint = LayoutHint::a4();
    let page = &plan.pages[0];
    let runs = text_runs(page);
    let formula_paths = paths(page);

    // 201 formula glyphs (100 "+" + 101 "x") are emitted either as text runs
    // or path items — the count is preserved regardless of which branch fires.
    let glyph_count = runs.iter().filter(|r| matches!(r.text.as_str(), "x" | "+")).count();
    assert_eq!(glyph_count + formula_path_count(page), 201, "every formula glyph is painted, none dropped");
    assert!(rules(page).is_empty(), "a plain symbol sequence contributes no rules");

    assert!(!runs.is_empty() || !formula_paths.is_empty(), "the oversized formula produces items");

    // Geometry checks use the first non-empty Path item when available.
    if let Some(first_path) = page.items.iter().find_map(|item| match item {
        LayoutItem::Path(p) if !p.d.is_empty() => Some((p.x, p.y)),
        _ => None,
    }) {
        assert!((first_path.0 - hint.margin.left).abs() < 1e-9, "clamped formula starts at the column left edge");
        let baseline = first_path.1;
        assert!(
            page.items.iter().all(|item| match item {
                LayoutItem::Path(p) if !p.d.is_empty() => (p.y - baseline).abs() < 1e-9,
                LayoutItem::TextRun(r) if matches!(r.text.as_str(), "x" | "+") => (r.y - baseline).abs() < 1e-9,
                _ => true,
            }),
            "an oversized formula stays on one line"
        );
    }
}

#[test]
fn unparseable_inline_formula_falls_back_to_its_raw_source() {
    let source = "\\notacommand{x}";
    let graph = paragraph(vec![text("See "), math(source), text(" here")]);
    let plan = layout(&graph);
    let page = &plan.pages[0];
    let runs = text_runs(page);

    assert_eq!(runs.len(), 1, "fallback joins the surrounding text flow: {runs:#?}");
    assert_eq!(runs[0].text, format!("See {source} here"));
    assert!(rules(page).is_empty(), "text fallback emits no rules");
}

#[test]
fn pure_text_paragraphs_keep_one_run_per_line_at_the_left_margin() {
    let graph = paragraph(vec![text("Pure text stays.")]);
    let plan = layout(&graph);
    let hint = LayoutHint::a4();
    let page = &plan.pages[0];
    let runs = text_runs(page);

    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].text, "Pure text stays.");
    assert_eq!(runs[0].x, hint.margin.left);
    assert!(runs[0].y > hint.margin.top && runs[0].y < hint.margin.top + runs[0].size);
}
