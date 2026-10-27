//! PDF gate for the shared math corpus.
//!
//! Math pages must be pure outlines: one fill per outlined glyph, rule and
//! standalone path in the layout tree, never a host text operator and never an
//! embedded host font. These checks read the PDF bytes directly, so they need no
//! Python. Two independent external parsers (pypdf and PyMuPDF) are exercised by
//! `scripts/ci/validate-math-corpus-pdf.py`, which drives the `#[ignore]` writer
//! below and is an optional gate instead of a hard test dependency.

use std::{env, fs, path::PathBuf};

use serde::Deserialize;
use vutex_layout::math::{LayoutNode, collect_layout_with_diagnostics, layout_formula};
use vutex_layout::plan::{LayoutItem, LayoutPage, LayoutPlan};
use vutex_presentation::build_display_presentation;
use vutex_render_pdf::render_pdf_with_diagnostics;

/// Margin around every formula page, in points.
const MARGIN: f64 = 24.0;

/// One formula record in the gate corpus.
#[derive(Debug, Deserialize)]
struct GateCase {
    src: String,
    #[allow(dead_code)]
    group: String,
    style: String,
    expected: String,
    #[allow(dead_code)]
    diagnostics: bool,
    #[allow(dead_code)]
    invariants: Vec<String>,
    #[allow(dead_code)]
    body: Option<String>,
}

/// The corpus document shipped with `vutex-layout`.
#[derive(Debug, Deserialize)]
struct GateCorpus {
    corpus: String,
    cases: Vec<GateCase>,
}

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

/// Lay one corpus case out, lower it to display primitives and render a page.
fn render_page(case: &GateCase) -> (Vec<u8>, Vec<LayoutItem>, LayoutNode) {
    let display = is_display(case);
    let (root, layout) = layout_formula(&case.src, display)
        .unwrap_or_else(|e| panic!("`{}` ({}) must lay out: {e}", case.src, case.style));

    let baseline = MARGIN + layout.height;
    let mut diagnostics = Vec::new();
    let items = collect_layout_with_diagnostics(&root, MARGIN, baseline, &mut diagnostics);
    assert!(diagnostics.is_empty(), "`{}`: the embedded font must outline every glyph: {diagnostics:?}", case.src);

    let plan = LayoutPlan {
        pages: vec![LayoutPage {
            width: layout.width + 2.0 * MARGIN,
            height: layout.height + layout.depth + 2.0 * MARGIN,
            items: items.clone(),
        }],
    };
    let display_presentation = build_display_presentation(&plan);
    let outcome = render_pdf_with_diagnostics(&display_presentation)
        .unwrap_or_else(|e| panic!("`{}`: pdf render failed: {e:?}", case.src));
    (outcome.pdf, items, root)
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
        let stream_start = body_start + text[body_start..].find("stream\n").expect("stream") + "stream\n".len();
        let stream_end = body_start + text[body_start..].rfind("\nendstream").expect("stream end");
        contents.push(text[stream_start..stream_end].to_string());
        cursor = body_end;
    }
    assert!(!contents.is_empty(), "the page carries a content stream");
    contents.join("\n")
}

/// Every corpus layout case lowers to pure outlines with no host text.
#[test]
fn every_corpus_formula_renders_paths_only() {
    let corpus = load_corpus();
    let mut violations = Vec::new();
    let mut checked = 0;

    for case in corpus.cases.iter().filter(|case| case.expected == "layout") {
        let (pdf, items, root) = render_page(case);
        checked += 1;
        let label = format!("`{}` ({})", case.src, case.style);
        let bytes = String::from_utf8_lossy(&pdf).into_owned();
        let content = page_content(&pdf);

        if !bytes.starts_with("%PDF-1.7") || !bytes.ends_with("%%EOF\n") {
            violations.push(format!("{label}: malformed PDF envelope"));
        }
        if !items.iter().all(|item| !matches!(item, LayoutItem::TextRun(_))) {
            violations.push(format!("{label}: a math glyph lowered to a text run: {items:#?}"));
        }
        if content.contains("BT\n") || content.contains(" Tj\n") || content.contains(" TJ\n") {
            violations.push(format!("{label}: the page opens a text object or a text operator:\n{content}"));
        }
        if bytes.contains("/BaseFont") {
            violations.push(format!("{label}: the page references a host font:\n{bytes}"));
        }

        let primitives = path_primitives(&root);
        if primitives == 0 {
            violations.push(format!("{label}: the layout tree paints nothing"));
        }
        if content.matches("\nf\n").count() != primitives {
            violations.push(format!(
                "{label}: expected one fill per glyph, rule and path ({primitives}), painted {}:\n{content}",
                content.matches("\nf\n").count()
            ));
        }
        if content.contains("\nS\n") {
            violations.push(format!("{label}: a math page strokes something:\n{content}"));
        }
    }

    assert!(checked >= 30, "the PDF gate must cover the corpus, checked {checked}");
    assert!(violations.is_empty(), "{} PDF gate failure(s):\n{}", violations.len(), violations.join("\n"));
}

/// Write one PDF per layout case so an external, dual-parser harness can read
/// them. `VUTEX_CORPUS_PDF_DIR` names the output directory.
#[test]
#[ignore = "runs only from scripts/ci/validate-math-corpus-pdf.py"]
fn write_corpus_pdfs_for_external_check() {
    let directory = env::var("VUTEX_CORPUS_PDF_DIR").expect("VUTEX_CORPUS_PDF_DIR must be set");
    let directory = PathBuf::from(directory);
    fs::create_dir_all(&directory).expect("create pdf output directory");

    let corpus = load_corpus();
    let mut manifest = Vec::new();
    let mut index = 0;

    for case in corpus.cases.iter().filter(|case| case.expected == "layout") {
        let (pdf, _, _) = render_page(case);
        let file = format!("case-{index:02}.pdf");
        fs::write(directory.join(&file), &pdf).unwrap_or_else(|e| panic!("write {file}: {e}"));
        manifest.push(serde_json::json!({
            "file": file,
            "src": case.src,
            "style": case.style,
            "bytes": pdf.len(),
        }));
        index += 1;
    }

    let manifest_path = directory.join("manifest.json");
    fs::write(&manifest_path, serde_json::to_vec_pretty(&manifest).expect("manifest json"))
        .unwrap_or_else(|e| panic!("write {}: {e}", manifest_path.display()));
    eprintln!("vutex: wrote {} corpus PDFs to {}", index, directory.display());
}
