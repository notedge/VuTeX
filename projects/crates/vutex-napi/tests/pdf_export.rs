//! External PDF export test.
//!
//! Marked `#[ignore]` so `cargo test` does not run it by default. A Python
//! harness drives it by setting three environment variables:
//!
//! - `VUTEX_INPUT` : absolute path to a markdown/tex/html/typst input file.
//! - `VUTEX_OUTPUT`: absolute path where the generated PDF bytes are written.
//! - `VUTEX_OPTIONS`: (optional) JSON string for render options, e.g.
//!   `{"font_policy":"filesystem"}`. Defaults to `{"output":"pdf"}`.
//!
//! The test asserts the render succeeds and writes the PDF bytes so that an
//! external parser (pymupdf, poppler, ...) can validate the file structure.

use std::{
    env,
    fs,
    path::{Path, PathBuf},
};

use vutex_napi::document::render_document;

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[test]
#[ignore = "runs only from external harness (VUTEX_INPUT/VUTEX_OUTPUT)"]
fn export_pdf_to_path() {
    let input = env::var("VUTEX_INPUT").expect("VUTEX_INPUT must be set");
    let output = env::var("VUTEX_OUTPUT").expect("VUTEX_OUTPUT must be set");
    let options = env::var("VUTEX_OPTIONS").ok();

    let input_path = PathBuf::from(&input);
    assert!(input_path.exists(), "input file does not exist: {input}");

    let mut opts = match options.as_deref() {
        Some(json) if !json.trim().is_empty() => serde_json::from_str::<serde_json::Value>(json)
            .expect("VUTEX_OPTIONS must be valid JSON"),
        _ => serde_json::Value::default(),
    };
    if opts.get("output").is_none() {
        opts["output"] = serde_json::Value::String("pdf".into());
    }

    let outcome = render_document(&path_string(&input_path), Some(&opts.to_string()));
    assert_eq!(outcome.exit_code, 0, "render failed: {}", outcome.report_json);
    assert_eq!(outcome.output_format, "pdf");

    let pdf = outcome.pdf.expect("pdf payload is present on success");
    assert!(!pdf.is_empty(), "pdf payload is non-empty");
    assert!(pdf.starts_with(b"%PDF-1.7"), "pdf header");
    assert!(pdf.ends_with(b"%%EOF\n"), "pdf trailer");

    let out_path = PathBuf::from(&output);
    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent).expect("create output directory");
    }
    fs::write(&out_path, &pdf).expect("write pdf bytes");

    eprintln!(
        "vutex: wrote {} bytes to {} (pages: {})",
        pdf.len(),
        out_path.display(),
        serde_json::from_str::<serde_json::Value>(&outcome.report_json)
            .ok()
            .and_then(|r| r.get("output").and_then(|o| o.get("pages")).and_then(|p| p.as_u64()))
            .unwrap_or(0),
    );
}
