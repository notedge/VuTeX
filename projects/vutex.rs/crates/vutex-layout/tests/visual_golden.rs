//! Visual golden manifest contract (gate N7-6).
//!
//! The goldens live in `tests/fixtures/visual/`: one PNG per corpus formula
//! per display mode, rasterized from the layout SVG by pinned headless Chrome
//! (version, flags, DPI and background recorded in `manifest.json`). The
//! pixel-level hash comparison runs in the Node harness
//! (`projects/vutex.ts/examples/visual-golden/capture.mjs --verify`), which
//! re-renders every formula and fails on any byte drift under the pinned
//! rasterizer. This test pins the structural half of the contract on the Rust
//! side: the manifest covers exactly the corpus, every golden file exists with
//! its recorded byte length, and the rasterizer identity stays pinned.
//!
//! `vutex-render-raster` remains an unimplemented stub; the golden rasterizer
//! is deliberately an external, version-pinned Chrome rather than a pretended
//! in-crate implementation.

use std::{fs, path::Path};

use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Manifest {
    schema_version: String,
    rasterizer: Rasterizer,
    renderer: String,
    formulas: usize,
    entries: std::collections::BTreeMap<String, Entry>,
}

#[derive(Debug, Deserialize)]
struct Rasterizer {
    engine: String,
    version: String,
    flags: Vec<String>,
    #[serde(rename = "deviceScaleFactor")]
    device_scale_factor: u32,
    background: String,
    #[serde(rename = "paddingPx")]
    padding_px: u32,
}

#[derive(Debug, Deserialize)]
struct Entry {
    tex: String,
    display: bool,
    file: String,
    window: [u32; 2],
    bytes: usize,
    sha256: String,
}

fn visual_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join("visual")
}

fn corpus_ids() -> Vec<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join("oracle").join("corpus.json");
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    let json: serde_json::Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("corpus.json must be valid JSON: {e}"));
    json["formulas"].as_array().expect("corpus formulas").iter().map(|f| f["id"].as_str().expect("id").to_string()).collect()
}

/// The escaping rule `capture.mjs` applies so case-insensitive file systems
/// cannot collide `vmatrix` with `Vmatrix`.
fn file_slug(id: &str) -> String {
    let mut slug = String::with_capacity(id.len());
    for c in id.chars() {
        if c.is_ascii_uppercase() {
            slug.push('!');
            slug.push(c.to_ascii_lowercase());
        }
        else {
            slug.push(c);
        }
    }
    slug
}

fn load_manifest() -> Manifest {
    let path = visual_dir().join("manifest.json");
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("manifest.json must be valid JSON: {e}"))
}

#[test]
fn manifest_covers_exactly_the_corpus_in_both_display_modes() {
    let manifest = load_manifest();
    assert_eq!(manifest.schema_version, "v0", "golden manifest stays on the `/v0` contract namespace");
    let ids = corpus_ids();
    assert_eq!(manifest.formulas, ids.len(), "the manifest counts the same corpus");
    assert_eq!(manifest.entries.len(), ids.len() * 2, "every formula owns an inline and a display golden");
    for id in &ids {
        for mode in ["inline", "display"] {
            let key = format!("{id}-{mode}");
            let entry = manifest.entries.get(&key).unwrap_or_else(|| panic!("golden `{key}` is missing from the manifest"));
            assert_eq!(entry.display, mode == "display", "`{key}` records its own display mode");
            assert!(!entry.tex.trim().is_empty(), "`{key}` keeps the rendered source for review");
            assert_eq!(entry.file, format!("{}.png", file_slug(&key)), "`{key}` uses the case-safe file slug");
            assert_eq!(entry.sha256.len(), 64, "`{key}` records a full SHA-256");
            assert!(entry.bytes > 0, "`{key}` records a non-empty raster");
            let png = visual_dir().join(&entry.file);
            let actual = fs::metadata(&png).unwrap_or_else(|e| panic!("golden {} must exist: {e}", png.display())).len() as usize;
            assert_eq!(actual, entry.bytes, "`{key}` byte length drifted from the manifest");
            // The raster viewport wraps the SVG viewport with symmetric padding.
            assert!(
                entry.window[0] >= 64 && entry.window[1] >= 64,
                "`{key}` keeps the minimum viewport the harness guarantees"
            );
        }
    }
}

#[test]
fn the_rasterizer_identity_is_pinned() {
    let manifest = load_manifest();
    assert_eq!(manifest.rasterizer.engine, "headless-chrome");
    assert!(
        manifest.rasterizer.version.chars().next().is_some_and(|c| c.is_ascii_digit()),
        "the rasterizer version is an explicit pin, not a channel name"
    );
    assert_eq!(manifest.rasterizer.device_scale_factor, 1, "DPI is pinned to device scale factor 1");
    assert_eq!(manifest.rasterizer.background, "#ffffff", "the canvas background is pinned");
    assert!(manifest.rasterizer.flags.contains(&"--headless".to_string()));
    assert!(manifest.rasterizer.flags.contains(&"--force-device-scale-factor=1".to_string()));
    assert!(manifest.rasterizer.flags.contains(&"--hide-scrollbars".to_string()));
    assert_eq!(manifest.rasterizer.padding_px, 8, "the frame padding is pinned");
    assert!(
        manifest.renderer.contains("@vutex/watex renderSvg"),
        "the renderer identity is recorded: {}",
        manifest.renderer
    );
}
