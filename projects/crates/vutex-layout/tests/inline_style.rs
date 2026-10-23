//! Inline emphasis layout tests: styled spans reach positioned text runs.

use notedown_ir::{Attributes, Block, DocumentGraph, DocumentId, Inline, NodeId, RelationEndpoint};
use vutex_layout::{
    LayoutHint,
    document::layout_with_hint,
    hint::Margin,
    layout,
    plan::{LayoutItem, LayoutPage, Rule, TextRun},
};
use vutex_types::math::FontStyle;

fn text(value: &str) -> Inline {
    Inline::Text { text: value.to_string() }
}

fn styled(style: &str, children: Vec<Inline>) -> Inline {
    Inline::Styled { style: style.to_string(), children }
}

fn code(value: &str) -> Inline {
    Inline::InlineCode { text: value.to_string() }
}

fn paragraph(content: Vec<Inline>) -> DocumentGraph {
    let mut graph = DocumentGraph::new(DocumentId(21));
    graph.push_block(Block::Paragraph { content });
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

#[test]
fn bold_span_becomes_its_own_text_run() {
    let graph = paragraph(vec![text("Start "), styled("bold", vec![text("thick")]), text(" end")]);
    let plan = layout(&graph);
    let runs = text_runs(&plan.pages[0]);

    assert_eq!(runs.len(), 3, "a style change must split the run: {runs:#?}");
    assert_eq!(runs[0].text, "Start ");
    assert_eq!(runs[0].style, FontStyle::Normal);
    assert_eq!(runs[1].text, "thick");
    assert_eq!(runs[1].style, FontStyle::Bold);
    assert_eq!(runs[1].font_family, "serif");
    assert_eq!(runs[2].text, " end");
    assert_eq!(runs[2].style, FontStyle::Normal);

    // Styled runs keep flowing on the same baseline, left to right.
    assert!(runs[1].x > runs[0].x);
    assert!(runs[2].x > runs[1].x);
    assert!(runs.iter().all(|run| (run.y - runs[0].y).abs() < 1e-9));
}

#[test]
fn strong_and_em_aliases_map_to_the_same_faces() {
    let graph = paragraph(vec![styled("strong", vec![text("heavy")]), text(" "), styled("em", vec![text("slanted")])]);
    let plan = layout(&graph);
    let runs = text_runs(&plan.pages[0]);

    assert_eq!(runs.len(), 3, "{runs:#?}");
    assert_eq!(runs[0].text, "heavy");
    assert_eq!(runs[0].style, FontStyle::Bold);
    assert_eq!(runs[1].text, " ");
    assert_eq!(runs[1].style, FontStyle::Normal);
    assert_eq!(runs[2].text, "slanted");
    assert_eq!(runs[2].style, FontStyle::Italic);
}

#[test]
fn inline_code_uses_the_monospace_family() {
    let graph = paragraph(vec![text("run "), code("npm test"), text(" now")]);
    let plan = layout(&graph);
    let runs = text_runs(&plan.pages[0]);

    assert_eq!(runs.len(), 3, "{runs:#?}");
    let code_run = runs.iter().find(|run| run.text == "npm test").expect("inline code run");
    assert_eq!(code_run.font_family, "monospace");
    assert_eq!(code_run.style, FontStyle::Normal);
    assert!(runs.iter().filter(|run| run.text != "npm test").all(|run| run.font_family == "serif"));
}

#[test]
fn inline_code_is_measured_with_monospace_advances() {
    let hint = LayoutHint::a4();
    let graph = paragraph(vec![text("run "), code("npm test"), text(" now")]);
    let plan = layout(&graph);
    let runs = text_runs(&plan.pages[0]);

    let code_run = runs.iter().find(|run| run.text == "npm test").expect("inline code run");
    let tail = runs.iter().find(|run| run.text == " now").expect("text after the code span");

    // Courier-class monospace advances 0.6 em per glyph, so the run that
    // follows the code span starts where the monospace glyphs actually end.
    let code_advance = 8.0 * 0.6 * hint.base_font_size;
    assert!((tail.x - code_run.x - code_advance).abs() < 1e-6, "code {} tail {}", code_run.x, tail.x);
}

#[test]
fn text_run_width_matches_the_measured_advance() {
    let hint = LayoutHint::a4();
    let graph = paragraph(vec![text("run "), code("npm test"), text(" now")]);
    let plan = layout(&graph);
    let runs = text_runs(&plan.pages[0]);

    let lead = runs.iter().find(|run| run.text == "run ").expect("leading text run");
    let code_run = runs.iter().find(|run| run.text == "npm test").expect("inline code run");
    let tail = runs.iter().find(|run| run.text == " now").expect("text after the code span");

    // The monospace run is 0.6 em per glyph wide, and each following run starts
    // exactly at the previous run's `x + width`.
    assert!((code_run.width - 8.0 * 0.6 * hint.base_font_size).abs() < 1e-6, "width {}", code_run.width);
    assert!((code_run.x - lead.x - lead.width).abs() < 1e-9, "lead {} then {}", lead.x, code_run.x);
    assert!((tail.x - code_run.x - code_run.width).abs() < 1e-9, "code {} tail {}", code_run.x, tail.x);
}

#[test]
fn mono_style_spans_also_use_the_monospace_family() {
    let graph = paragraph(vec![styled("code", vec![text("verbatim")])]);
    let plan = layout(&graph);
    let runs = text_runs(&plan.pages[0]);

    assert_eq!(runs.len(), 1, "{runs:#?}");
    assert_eq!(runs[0].text, "verbatim");
    assert_eq!(runs[0].font_family, "monospace");
    assert_eq!(runs[0].style, FontStyle::Monospace);
}

#[test]
fn unknown_style_names_stay_body_text() {
    let graph = paragraph(vec![styled("highlight", vec![text("plain")])]);
    let plan = layout(&graph);
    let runs = text_runs(&plan.pages[0]);

    assert_eq!(runs.len(), 1, "{runs:#?}");
    assert_eq!(runs[0].text, "plain");
    assert_eq!(runs[0].style, FontStyle::Normal);
    assert_eq!(runs[0].font_family, "serif");
}

#[test]
fn styled_runs_still_wrap_inside_the_content_width() {
    let hint = LayoutHint::a4().with_page_size(320.0, 600.0).with_margin(Margin::uniform(40.0));
    let body = "word ".repeat(40);
    let graph = paragraph(vec![text("Lead "), styled("bold", vec![text(body.trim())]), text(" tail")]);
    let plan = layout_with_hint(&graph, &hint);
    let runs = text_runs(&plan.pages[0]);

    let bold: Vec<&TextRun> = runs.iter().copied().filter(|run| run.style == FontStyle::Bold).collect();
    assert!(bold.len() > 1, "the styled span must wrap onto several lines: {bold:#?}");
    let baselines: std::collections::HashSet<String> = bold.iter().map(|run| format!("{:.3}", run.y)).collect();
    assert!(baselines.len() > 1, "wrapped styled runs must sit on distinct baselines: {bold:#?}");

    assert!(runs.iter().all(|run| run.x >= hint.margin.left - 1e-9), "runs start inside the left margin");
    assert!(
        runs.iter().all(|run| run.x <= hint.page_width - hint.margin.right + 1e-9),
        "runs never start past the right margin: {runs:#?}"
    );

    // The unstyled lead and tail keep their own runs.
    assert!(runs.iter().any(|run| run.style == FontStyle::Normal && run.text.starts_with("Lead")));
    assert!(runs.iter().any(|run| run.style == FontStyle::Normal && run.text.contains("tail")));
}

#[test]
fn unstyled_paragraphs_keep_one_run_per_line() {
    let graph = paragraph(vec![text("Pure text stays.")]);
    let plan = layout(&graph);
    let runs = text_runs(&plan.pages[0]);

    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].text, "Pure text stays.");
    assert_eq!(runs[0].style, FontStyle::Normal);
    assert_eq!(runs[0].font_family, "serif");
}

#[test]
fn strike_span_emits_a_rule_across_the_run() {
    let graph = paragraph(vec![text("keep "), styled("strike", vec![text("gone")]), text(" later")]);
    let plan = layout(&graph);
    let page = &plan.pages[0];
    let runs = text_runs(page);
    let strike_run = runs.iter().find(|run| run.text == "gone").expect("strike run");
    let tail = runs.iter().find(|run| run.text == " later").expect("run after the strike span");

    assert_eq!(strike_run.style, FontStyle::Strike);
    assert_eq!(strike_run.font_family, "serif");

    let decorations = rules(page);
    assert_eq!(decorations.len(), 1, "only the strike run draws a decoration: {decorations:#?}");
    let rule = decorations[0];
    assert!((rule.y1 - rule.y2).abs() < 1e-9, "the strike rule is horizontal: {rule:?}");

    let baseline = strike_run.y;
    let size = strike_run.size;
    assert!(
        rule.y1 > baseline - 0.5 * size && rule.y1 < baseline - 0.1 * size,
        "the rule crosses the run between 0.1 and 0.5 em above the baseline: {rule:?}"
    );

    // The run after the span starts where the strike glyphs end, so the
    // decoration spans exactly the measured run width.
    assert!((rule.x1 - strike_run.x).abs() < 1e-9, "the rule starts at the run: {rule:?}");
    assert!(
        (rule.x2 - rule.x1 - (tail.x - strike_run.x)).abs() < 1e-9,
        "rule {rule:?} must cover the run {strike_run:?} up to {tail:?}"
    );
}

#[test]
fn wrapped_children_keep_their_styles() {
    let graph = paragraph(vec![
        Inline::Attributed { attributes: Attributes::default(), children: vec![styled("bold", vec![text("kept")])] },
        Inline::Link {
            target: RelationEndpoint::ExternalUri("https://example.com".to_string()),
            display: vec![text(" see "), styled("italic", vec![text("note")])],
            title: None,
        },
        Inline::Note { label: "1".to_string(), display: Some(vec![styled("bold", vec![text("foot")])]) },
    ]);
    let plan = layout(&graph);
    let runs = text_runs(&plan.pages[0]);

    let kept = runs.iter().find(|run| run.text == "kept").expect("attributed child run");
    assert_eq!(kept.style, FontStyle::Bold);
    let note = runs.iter().find(|run| run.text == "note").expect("link display run");
    assert_eq!(note.style, FontStyle::Italic);
    let foot = runs.iter().find(|run| run.text == "foot").expect("note display run");
    assert_eq!(foot.style, FontStyle::Bold);
}

#[test]
fn external_link_text_run_carries_href() {
    let graph = paragraph(vec![Inline::Link {
        target: RelationEndpoint::ExternalUri("https://example.com".to_string()),
        display: vec![text("guide")],
        title: None,
    }]);
    let plan = layout(&graph);
    let runs = text_runs(&plan.pages[0]);

    assert_eq!(runs.len(), 1, "{runs:#?}");
    assert_eq!(runs[0].text, "guide");
    assert_eq!(runs[0].href.as_deref(), Some("https://example.com"));
}

#[test]
fn internal_link_text_run_has_no_href() {
    let graph = paragraph(vec![Inline::Link {
        target: RelationEndpoint::Node(NodeId(7)),
        display: vec![text("Section seven")],
        title: None,
    }]);
    let plan = layout(&graph);
    let runs = text_runs(&plan.pages[0]);

    assert_eq!(runs.len(), 1, "{runs:#?}");
    assert_eq!(runs[0].text, "Section seven");
    assert_eq!(runs[0].href, None, "internal targets carry no destination");
}

#[test]
fn link_text_does_not_merge_with_adjacent_text() {
    let graph = paragraph(vec![
        text("Read "),
        Inline::Link {
            target: RelationEndpoint::ExternalUri("https://example.com".to_string()),
            display: vec![text("the guide")],
            title: None,
        },
        text(" now"),
    ]);
    let plan = layout(&graph);
    let runs = text_runs(&plan.pages[0]);

    // The destination is part of the run key, so the plain neighbours keep
    // their own runs instead of being pulled inside the anchor.
    assert_eq!(runs.len(), 3, "{runs:#?}");
    assert_eq!(runs[0].text, "Read ");
    assert_eq!(runs[0].href, None);
    assert_eq!(runs[1].text, "the guide");
    assert_eq!(runs[1].href.as_deref(), Some("https://example.com"));
    assert_eq!(runs[2].text, " now");
    assert_eq!(runs[2].href, None);
}

#[test]
fn external_link_text_run_carries_the_authored_title() {
    let graph = paragraph(vec![Inline::Link {
        target: RelationEndpoint::ExternalUri("https://example.com".to_string()),
        display: vec![text("the "), styled("bold", vec![text("guide")]), code("now")],
        title: Some("Read the guide".to_string()),
    }]);
    let plan = layout(&graph);
    let runs = text_runs(&plan.pages[0]);

    assert_eq!(runs.len(), 3, "styles still split the link text: {runs:#?}");
    assert!(runs.iter().all(|run| run.title.as_deref() == Some("Read the guide")), "{runs:#?}");
    assert!(runs.iter().all(|run| run.href.as_deref() == Some("https://example.com")), "{runs:#?}");
}

#[test]
fn link_runs_with_different_titles_do_not_merge() {
    let graph = paragraph(vec![
        Inline::Link {
            target: RelationEndpoint::ExternalUri("https://example.com".to_string()),
            display: vec![text("first")],
            title: Some("First hint".to_string()),
        },
        Inline::Link {
            target: RelationEndpoint::ExternalUri("https://example.com".to_string()),
            display: vec![text("second")],
            title: Some("Second hint".to_string()),
        },
        Inline::Link {
            target: RelationEndpoint::ExternalUri("https://example.com".to_string()),
            display: vec![text("third")],
            title: Some(String::new()),
        },
    ]);
    let plan = layout(&graph);
    let runs = text_runs(&plan.pages[0]);

    // The hover text is part of the run key, so neighbouring anchors with the
    // same destination keep their own runs and tooltips.
    assert_eq!(runs.len(), 3, "{runs:#?}");
    assert_eq!(runs[0].text, "first");
    assert_eq!(runs[0].title.as_deref(), Some("First hint"));
    assert_eq!(runs[1].text, "second");
    assert_eq!(runs[1].title.as_deref(), Some("Second hint"));
    assert_eq!(runs[2].text, "third");
    assert_eq!(runs[2].title, None, "an empty authored title carries no tooltip");
    assert!(runs.iter().all(|run| run.href.as_deref() == Some("https://example.com")));
}

#[test]
fn unstyled_text_has_no_strike_rule() {
    let graph = paragraph(vec![text("Pure text stays.")]);
    let plan = layout(&graph);

    assert!(rules(&plan.pages[0]).is_empty(), "plain paragraphs draw no decoration: {:#?}", rules(&plan.pages[0]));
}
