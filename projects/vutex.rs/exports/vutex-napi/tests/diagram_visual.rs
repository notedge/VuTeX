//! Programmatic visual acceptance for lowered DOT and PlantUML diagrams.
//!
//! The Rust-side tests assert the SVG the pipeline emits; this file asserts the
//! pixels that renderer produces. The Chromium build Playwright bundles
//! rasterizes the scene SVG the snapshot backend actually embeds, counts
//! non-white pixels through a `<canvas>`, and writes a PNG screenshot. No human
//! opens a browser, and no browser already present on the host is ever used.
//!
//! The executable is resolved from `@playwright/test` in the TypeScript
//! workspace and its version is checked against the one recorded in the formula
//! golden manifest, so both harnesses share a single rasterizer identity.
//! `npx playwright install chromium` provisions the browser. The test is
//! `#[ignore]`d because CI runners do not install browsers; run it on a
//! workstation that has the workspace dependencies with:
//!
//! ```text
//! cargo test -p vutex-napi --test diagram_visual -- --ignored
//! ```

use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

use vutex_napi::document::render_document;

/// Directory that owns the Playwright install.
const GOLDEN_HARNESS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../vutex.ts/examples/visual-golden");

/// Fixed rasterizer flags; the viewport scale, background and scrollbars are
/// pinned so two runs of the same build produce identical rasters.
const CHROME_FLAGS: &[&str] = &[
    "--headless",
    "--disable-gpu",
    "--no-sandbox",
    "--hide-scrollbars",
    "--force-device-scale-factor=1",
    "--virtual-time-budget=2000",
];

const DOT_SAMPLE: &str = r##"# 有向图

```dot
digraph pipeline {
  rankdir=LR
  Fetch [shape=box]
  Store [shape=box]
  Fetch -> Store [label="persist"]
}
```
"##;

const PLANTUML_SAMPLE: &str = r##"# 类图

```plantuml
@startuml
class Order {
  +id : String
}
class Payable
Order o-- Payable
@enduml
```
"##;

/// Unique scratch directory per run.
struct Scratch {
    path: PathBuf,
}

impl Scratch {
    fn new(name: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let sequence = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("vutex-diagram-visual-{}-{name}-{sequence}", std::process::id()));
        std::fs::create_dir_all(&path).expect("create scratch directory");
        Self { path }
    }

    fn write(&self, name: &str, content: &str) -> PathBuf {
        let path = self.path.join(name);
        std::fs::write(&path, content).expect("write scratch file");
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// The Playwright-bundled Chromium executable.
///
/// Resolving it through the installed `@playwright/test` keeps the browser
/// version pinned by the workspace instead of whatever the host happens to have
/// installed.
fn bundled_chromium() -> PathBuf {
    let output = Command::new("node")
        .current_dir(GOLDEN_HARNESS)
        .args(["-e", "process.stdout.write(require('@playwright/test').chromium.executablePath())"])
        .output()
        .expect("node must be on PATH: the rasterizer comes from `@playwright/test`");
    assert!(
        output.status.success(),
        "resolving the Playwright Chromium failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let path = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    assert!(
        path.exists(),
        "Playwright's bundled Chromium is missing at {}; run `npx playwright install chromium` in {GOLDEN_HARNESS}",
        path.display()
    );
    assert!(
        path.to_string_lossy().contains("ms-playwright"),
        "`{}` is not a Playwright-bundled browser, and this harness must never drive a host browser",
        path.display()
    );
    path
}

/// The Chromium version the formula goldens were captured with.
fn pinned_chromium_version() -> String {
    let manifest = concat!(env!("CARGO_MANIFEST_DIR"), "/../../crates/vutex-layout/tests/fixtures/visual/manifest.json");
    let text = std::fs::read_to_string(manifest).expect("the formula golden manifest records the pinned rasterizer");
    let value: serde_json::Value = serde_json::from_str(&text).expect("the formula golden manifest is JSON");
    value["rasterizer"]["chromiumVersion"]
        .as_str()
        .expect("the formula golden manifest records `rasterizer.chromiumVersion`")
        .to_string()
}

/// Probe the bundled binary and refuse to mix rasters from a different build.
fn assert_pinned_chromium(chrome: &PathBuf, pinned: &str) {
    let probed = Command::new(chrome)
        .arg("--version")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|text| {
            let start = text.find(|c: char| c.is_ascii_digit())?;
            let end = start + text[start..].find(|c: char| !c.is_ascii_digit() && c != '.').unwrap_or(text.len() - start);
            Some(text[start..end].to_string())
        });
    if let Some(probed) = probed {
        assert_eq!(
            probed, pinned,
            "the bundled Chromium runs {probed} while the goldens pin {pinned}; re-capture deliberately instead of mixing pixels"
        );
    }
}

/// The scene SVG the snapshot backend embedded, sliced out verbatim.
fn embedded_svg(html: &str) -> &str {
    let start = html.find("<svg").expect("the snapshot embeds the scene svg");
    let end = html[start..].find("</svg>").expect("the scene svg closes") + start + "</svg>".len();
    &html[start..end]
}

/// Read a numeric SVG attribute such as `width` or `height`.
fn svg_attribute<'a>(svg: &'a str, name: &str) -> &'a str {
    let key = format!(" {name}=\"");
    let start = svg.find(&key).unwrap_or_else(|| panic!("the scene svg carries `{name}`: {svg}")) + key.len();
    let end = start + svg[start..].find('"').expect("attribute value closes");
    &svg[start..end]
}

/// Read the PNG width and height from the `IHDR` chunk.
fn png_extent(bytes: &[u8]) -> (u32, u32) {
    assert!(bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]), "the bundled Chromium must emit a PNG");
    assert_eq!(&bytes[12..16], b"IHDR", "the first chunk is IHDR");
    let width = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    let height = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    (width, height)
}

/// Wrap the scene SVG in a page that rasterizes it to a canvas and reports ink.
fn measurement_page(svg: &str, width: u32, height: u32) -> String {
    format!(
        concat!(
            "<!doctype html><html><head><meta charset=\"utf-8\">",
            "<style>html,body{{margin:0;padding:0;background:#ffffff}}</style></head><body>",
            "<div id=\"frame\" style=\"line-height:0\">{svg}</div>",
            "<canvas id=\"canvas\"></canvas><pre id=\"result\">pending</pre>",
            "<script>",
            "const width={width},height={height};",
            "const svg=document.querySelector('#frame svg');",
            "const canvas=document.querySelector('#canvas');",
            "canvas.width=width;canvas.height=height;",
            "const image=new Image();",
            "image.onload=()=>{{",
            "const context=canvas.getContext('2d');",
            "context.fillStyle='#ffffff';context.fillRect(0,0,width,height);",
            "context.drawImage(image,0,0,width,height);",
            "const pixels=context.getImageData(0,0,width,height).data;",
            "let ink=0;",
            "for(let index=0;index<pixels.length;index+=4){{",
            "if(pixels[index]<250||pixels[index+1]<250||pixels[index+2]<250){{ink+=1;}}",
            "}}",
            "document.querySelector('#result').textContent=`${{ink}} ${{width*height}} ${{width}} ${{height}}`;",
            "}};",
            "image.src='data:image/svg+xml;charset=utf-8,'+encodeURIComponent(new XMLSerializer().serializeToString(svg));",
            "</script></body></html>"
        ),
        svg = svg,
        width = width,
        height = height
    )
}

fn file_url(path: &Path) -> String {
    format!("file:///{}", path.to_string_lossy().replace('\\', "/"))
}

/// Rasterize one fixture scene and assert the pixels and the screenshot.
fn assert_scene_rasterizes(name: &str, label: &str, source: &str) {
    let chrome = bundled_chromium();
    assert_pinned_chromium(&chrome, &pinned_chromium_version());
    let scratch = Scratch::new(name);

    let input = scratch.write("pipeline.md", source);
    let outcome = render_document(&input.to_string_lossy(), Some(r#"{"output":"snapshot"}"#));
    assert_eq!(outcome.exit_code, 0, "{}", outcome.report_json);
    let html = outcome.html.expect("snapshot payload");
    assert!(html.contains(&format!(r#"data-diagram="{label}""#)), "the snapshot carries the lowered {label} scene: {html}");

    let svg = embedded_svg(&html);
    let width = svg_attribute(svg, "width").parse::<f64>().map(|extent| extent.ceil() as u32).expect("svg width is a number");
    let height =
        svg_attribute(svg, "height").parse::<f64>().map(|extent| extent.ceil() as u32).expect("svg height is a number");
    assert!(width > 0 && height > 0, "the scene canvas is positive: {width}x{height}");

    let page = scratch.write("measure.html", &measurement_page(svg, width, height));
    let output = Command::new(&chrome)
        .args(CHROME_FLAGS)
        .arg(format!("--user-data-dir={}", scratch.path.join("profile-measure").display()))
        .arg("--dump-dom")
        .arg(file_url(&page))
        .output()
        .expect("run pinned headless Chrome");
    assert!(output.status.success(), "chrome failed: {:?}", output.status);
    let dom = String::from_utf8_lossy(&output.stdout);
    let measured = dom
        .split("id=\"result\">")
        .nth(1)
        .and_then(|tail| tail.split("</pre>").next())
        .unwrap_or_else(|| panic!("the measurement page reports its ink: {dom}"))
        .trim();
    let numbers = measured.split_whitespace().map(|value| value.parse::<u64>()).collect::<Result<Vec<_>, _>>();
    let numbers = numbers.unwrap_or_else(|_| panic!("the measurement page must report numbers, got `{measured}`"));
    assert_eq!(numbers.len(), 4, "the measurement page reports ink, total, width, height: `{measured}`");
    let (ink, total, measured_width, measured_height) = (numbers[0], numbers[1], numbers[2], numbers[3]);
    assert_eq!((measured_width as u32, measured_height as u32), (width, height), "the raster keeps the scene canvas");
    assert!(ink > 0, "the lowered diagram must paint pixels, got {ink} of {total}");
    let ratio = ink as f64 / total as f64;
    assert!(ratio > 0.01, "the diagram must cover a visible share of the canvas, got {ratio:.4} ({ink}/{total})");
    assert!(ratio < 0.90, "the diagram must not flood the canvas, got {ratio:.4} ({ink}/{total})");
    println!("{name} raster: {ink}/{total} ink pixels ({ratio:.4}) on {width}x{height}");

    // A real screenshot artifact, not just a computed count.
    let window_width = width.max(400);
    let window_height = height.max(300);
    let screenshot = scratch.path.join(format!("{name}-snapshot.png"));
    let shot = Command::new(&chrome)
        .args(CHROME_FLAGS)
        .arg(format!("--window-size={window_width},{window_height}"))
        .arg(format!("--user-data-dir={}", scratch.path.join("profile-shot").display()))
        .arg(format!("--screenshot={}", screenshot.display()))
        .arg(file_url(&page))
        .output()
        .expect("run pinned headless Chrome screenshot");
    assert!(shot.status.success(), "chrome screenshot failed: {:?}", shot.status);
    let png = std::fs::read(&screenshot).expect("the screenshot exists");
    let (png_width, png_height) = png_extent(&png);
    assert_eq!((png_width, png_height), (window_width, window_height), "the screenshot keeps the requested viewport");
    assert!(png.len() > 1024, "a blank frame compresses below this; got {} bytes", png.len());
    println!("{name} screenshot: {} bytes at {png_width}x{png_height}", png.len());
}

#[test]
#[ignore = "requires the pinned headless Chrome build (see module docs)"]
fn dot_scene_rasterizes_with_visible_ink_and_a_png_screenshot() {
    assert_scene_rasterizes("dot", "DOT graph", DOT_SAMPLE);
}

#[test]
#[ignore = "requires the pinned headless Chrome build (see module docs)"]
fn plantuml_scene_rasterizes_with_visible_ink_and_a_png_screenshot() {
    assert_scene_rasterizes("plantuml", "PlantUML diagram", PLANTUML_SAMPLE);
}
