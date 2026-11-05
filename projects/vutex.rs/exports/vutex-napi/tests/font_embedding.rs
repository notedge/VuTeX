//! Font embedding lane for the SVG document backend.
//!
//! `document-renderer-svg` names generic CSS families, so the host decides which
//! face paints each run. That face can be wider than the one `document-layout`
//! measured with, which overlaps runs. These tests pin the payload to the
//! bundled `document_fonts::BODY_FACES` instead: explicit family names plus the
//! face bytes, so the painting face is the measured face.

use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use vutex_napi::{document::render_document, svg_fonts::embed_body_fonts};

struct ScratchDir {
    path: PathBuf,
}

impl ScratchDir {
    fn new(name: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let sequence = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("vutex-fonts-{}-{name}-{sequence}", std::process::id()));
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

fn render_svg(name: &str, source: &str) -> String {
    let dir = ScratchDir::new(name);
    let path = dir.write("sample.md", source);
    let outcome = render_document(&path_string(&path), None);
    assert_eq!(outcome.exit_code, 0, "{}", outcome.report_json);
    outcome.svg.expect("svg payload")
}

#[test]
fn latin_document_carries_the_latin_and_mono_faces_only() {
    let svg = render_svg("latin", "# Title\n\nBody prose with `code_span` inside.\n");

    assert!(svg.contains("<style type=\"text/css\">"), "no face block");
    assert_eq!(svg.matches("@font-face").count(), 4, "Tinos regular, bold, italic, and Noto Sans Mono");
    assert!(svg.contains("font-family=\"VuTeX Serif\""), "prose run not named");
    assert!(svg.contains("font-family=\"VuTeX Mono\""), "code run not named");
    assert!(!svg.contains("font-family=\"serif\""), "generic prose family left in place");
    assert!(!svg.contains("font-family=\"monospace\""), "generic code family left in place");
    assert!(!svg.contains("VuTeX Serif CJK"), "Latin payload carries the CJK faces");
}

#[test]
fn han_document_carries_the_cjk_chain() {
    let svg = render_svg("han", "# 标题\n\n这是一段中文正文，包含标点。\n");

    assert_eq!(svg.matches("@font-face").count(), 6, "three Latin, two CJK, and one mono face");
    assert!(svg.contains("VuTeX Serif CJK"), "CJK family missing");
    assert!(svg.contains("font-family=\"VuTeX Serif, VuTeX Serif CJK\""), "prose run has no CJK fallback");
}

#[test]
fn every_generic_family_tag_is_rewritten() {
    let source = "<svg xmlns=\"http://www.w3.org/2000/svg\"><title>x</title><text font-family=\"serif\">a</text><text font-family=\"monospace\">b</text></svg>";
    let out = embed_body_fonts(source);
    assert!(!out.contains("\"serif\""), "{out}");
    assert!(!out.contains("\"monospace\""), "{out}");
}
