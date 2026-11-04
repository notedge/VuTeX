//! Smoke lane for the Node-API document binding.
//!
//! The binding is a thin wrapper: import, layout, presentation lowering and
//! rendering all live in the `document` family, which owns the per-lane
//! coverage tests. What can still break here is the wires between that family
//! and this crate, so this lane only proves the crate compiles and that one
//! minimal source per input format reaches an SVG payload, plus the refusal
//! paths that must never return a payload.

use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use serde_json::Value;
use vutex_napi::document::render_document;

/// Unique scratch directory per test, removed by [`ScratchDir`] on drop.
struct ScratchDir {
    path: PathBuf,
}

impl ScratchDir {
    fn new(name: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let sequence = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("vutex-napi-smoke-{}-{name}-{sequence}", std::process::id()));
        std::fs::create_dir_all(&path).expect("create scratch directory");
        Self { path }
    }

    fn write(&self, file_name: &str, content: &str) -> PathBuf {
        let path = self.path.join(file_name);
        std::fs::write(&path, content).expect("write scratch file");
        path
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn report(outcome: &vutex_napi::document::RenderOutcome) -> Value {
    serde_json::from_str(&outcome.report_json).expect("render report is valid JSON")
}

/// Assert one minimal source renders an SVG payload through the binding.
fn assert_svg_lane(name: &str, file_name: &str, source: &str, format: &str) {
    let dir = ScratchDir::new(name);
    let path = dir.write(file_name, source);

    let outcome = render_document(&path_string(&path), None);
    let payload = report(&outcome);

    assert_eq!(outcome.exit_code, 0, "{}", outcome.report_json);
    assert_eq!(outcome.output_format, "svg");
    assert_eq!(payload["inputs"][0]["format"], format, "{payload}");
    let svg = outcome.svg.expect("svg payload");
    assert!(svg.contains("<svg"), "{svg}");
}

#[test]
fn markdown_source_reaches_svg() {
    assert_svg_lane("markdown", "sample.md", "# Title\n\nHello world.\n", "markdown");
}

#[test]
fn tex_source_reaches_svg() {
    assert_svg_lane("tex", "sample.tex", "\\section{Introduction}\n\nHello world.\n", "tex");
}

#[test]
fn typst_source_reaches_svg() {
    assert_svg_lane("typst", "sample.typ", "= Introduction\n\nHello world.\n", "typst");
}

#[test]
fn absent_input_is_refused_without_a_payload() {
    let dir = ScratchDir::new("absent");

    let outcome = render_document(&path_string(&dir.path.join("absent.md")), None);

    assert_eq!(outcome.exit_code, 1);
    assert!(outcome.svg.is_none());
    assert!(outcome.html.is_none());
    assert!(outcome.report_json.contains("VUTEX_INPUT_NOT_FOUND"), "{}", outcome.report_json);
}

#[test]
fn unsupported_output_is_refused_without_a_payload() {
    let dir = ScratchDir::new("bad-output");
    let path = dir.write("sample.md", "# Title\n\nHello world.\n");

    let outcome = render_document(&path_string(&path), Some(r#"{"output":"docx"}"#));

    assert_eq!(outcome.exit_code, 1);
    assert!(outcome.pdf.is_none());
    assert!(outcome.report_json.contains("VUTEX_OUTPUT_FORMAT_UNSUPPORTED"), "{}", outcome.report_json);
}
