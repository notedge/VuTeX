#![deny(missing_docs)]

//! Node-API export surface for VuTeX.
//!
//! The binding is a library. Every export is a fine-grained primitive with
//! data-in and data-out semantics:
//!
//! * [`parse_document`] takes source bytes and returns a graph payload,
//! * [`lower_document`] takes a graph and returns a lowering handle,
//! * [`build_display`] and [`build_flow`] turn a lowering or graph into a scene,
//! * the scene handles emit SVG, snapshot HTML, reflow HTML, and PDF bytes,
//! * [`embed_svg_fonts`] and [`subset_face`] inject and subset bundled faces,
//! * [`summarize_document`] and [`assemble_report`] build report data.
//!
//! No export reads an input path, writes an output file, prints a message, or
//! returns a process exit code. Command-line behavior lives in the TypeScript
//! package, which composes these calls.

mod assets;

pub mod diagram;
pub mod document;
pub mod font_subset;
pub mod footnote;
pub mod svg_fonts;
pub mod theme;
pub mod tikz;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Mutex, OnceLock};

use document_fonts::{BodyFace, BodyFaceStyle, BodyFamily, BodyScript};
use document_ir::DocumentGraph;
use napi::bindgen_prelude::Buffer;
use napi_derive::napi;
use vutex_layout::hint::Align;
use vutex_text::math::MathFontFamily;
use watex_wasm::{RenderOptions, measure, render, render_mathml, render_svg};

use crate::theme::Theme;

/// Install the crate's math faces once per process.
fn install_embedded_math_fonts() {
    document_fonts::install_math_fonts(&assets::math_pack());
}

/// Intern a template-supplied string for the `'static` layout hint.
///
/// A header literal lives for the life of the process, so a host that renders
/// many documents with the same template does not leak one copy per render.
fn intern_template_text(value: String) -> &'static str {
    static INTERNED: OnceLock<Mutex<BTreeMap<String, &'static str>>> = OnceLock::new();
    let map = INTERNED.get_or_init(|| Mutex::new(BTreeMap::new()));
    let mut guard = map.lock().expect("template text intern");
    if let Some(existing) = guard.get(&value) {
        return existing;
    }
    let leaked: &'static str = Box::leak(value.clone().into_boxed_str());
    guard.insert(value, leaked);
    leaked
}

/// PDF bytes plus render diagnostics.
#[napi(object)]
pub struct PdfRenderResponse {
    /// Rendered PDF program.
    pub pdf: Buffer,
    /// Serialized render diagnostics.
    pub diagnostics_json: String,
}

/// Returns the VuTeX N-API binding version.
#[napi]
pub fn vutex_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Measure a formula and return JSON (`FormulaRender`).
#[napi]
pub fn measure_formula(source: String, options_json: Option<String>) -> String {
    install_embedded_math_fonts();
    let options = parse_render_options(options_json);
    serde_json::to_string(&measure(&source, &options)).unwrap_or_else(|_| "{}".to_string())
}

/// Render a formula to SVG and return JSON (`FormulaRender`).
#[napi]
pub fn render_formula_svg(source: String, options_json: Option<String>) -> String {
    install_embedded_math_fonts();
    let options = parse_render_options(options_json);
    serde_json::to_string(&render_svg(&source, &options)).unwrap_or_else(|_| "{}".to_string())
}

/// Render a formula to MathML and return JSON (`FormulaRender`).
#[napi]
pub fn render_formula_mathml(source: String, options_json: Option<String>) -> String {
    install_embedded_math_fonts();
    let options = parse_render_options(options_json);
    serde_json::to_string(&render_mathml(&source, &options)).unwrap_or_else(|_| "{}".to_string())
}

/// Render a formula with SVG and MathML and return JSON (`FormulaRender`).
#[napi]
pub fn render_formula(source: String, options_json: Option<String>) -> String {
    install_embedded_math_fonts();
    let options = parse_render_options(options_json);
    serde_json::to_string(&render(&source, &options)).unwrap_or_else(|_| "{}".to_string())
}

fn parse_render_options(raw: Option<String>) -> RenderOptions {
    match raw {
        Some(json) if !json.trim().is_empty() => serde_json::from_str(&json).unwrap_or_default(),
        _ => RenderOptions::default(),
    }
}

/// Parse source bytes into a document graph.
///
/// Returns `{"ok":true,"format":…,"graph":…,"diagnostics":[]}` on success and
/// `{"ok":false,"format":…,"diagnostics":[record]}` on failure. The caller owns
/// reading the bytes and deciding what a failure means for its own exit code.
#[napi]
pub fn parse_document(label: String, format: String, bytes: Buffer) -> String {
    install_embedded_math_fonts();
    let import_format = document::ImportFormat::from_option(&format);
    match document::parse_document(&label, import_format, &bytes) {
        Ok(parsed) => serde_json::json!({
            "ok": true,
            "format": parsed.format.id(),
            "graph": parsed.graph,
            "diagnostics": parsed.diagnostics,
        })
        .to_string(),
        Err(record) => serde_json::json!({
            "ok": false,
            "format": import_format.id(),
            "diagnostics": [record],
        })
        .to_string(),
    }
}

/// Lower diagrams and footnotes from a serialized document graph.
#[napi]
pub fn lower_document(graph_json: String, label: String) -> napi::Result<LoweredDocumentHandle> {
    let graph: DocumentGraph = serde_json::from_str(&graph_json)
        .map_err(|failure| napi::Error::from_reason(format!("document graph is not valid JSON: {failure}")))?;
    Ok(LoweredDocumentHandle { inner: Mutex::new(Some(document::lower_document(graph, &label))) })
}

/// Lowered document held between the lowering and layout primitives.
#[napi]
pub struct LoweredDocumentHandle {
    inner: Mutex<Option<document::LoweredDocument>>,
}

#[napi]
impl LoweredDocumentHandle {
    /// Serialized graph after footnote projection.
    #[napi]
    pub fn graph_json(&self) -> napi::Result<String> {
        let guard = self.inner.lock().map_err(|_| napi::Error::from_reason("lowered document is poisoned"))?;
        let lowered = guard.as_ref().ok_or_else(|| napi::Error::from_reason("lowered document was consumed"))?;
        serde_json::to_string(&lowered.graph)
            .map_err(|failure| napi::Error::from_reason(format!("graph serialization failed: {failure}")))
    }

    /// Serialized lowering diagnostics.
    #[napi]
    pub fn diagnostics_json(&self) -> napi::Result<String> {
        let guard = self.inner.lock().map_err(|_| napi::Error::from_reason("lowered document is poisoned"))?;
        let lowered = guard.as_ref().ok_or_else(|| napi::Error::from_reason("lowered document was consumed"))?;
        serde_json::to_string(&lowered.diagnostics)
            .map_err(|failure| napi::Error::from_reason(format!("diagnostics serialization failed: {failure}")))
    }

    /// Serialized footnote counts.
    #[napi]
    pub fn footnotes_json(&self) -> napi::Result<String> {
        let guard = self.inner.lock().map_err(|_| napi::Error::from_reason("lowered document is poisoned"))?;
        let lowered = guard.as_ref().ok_or_else(|| napi::Error::from_reason("lowered document was consumed"))?;
        serde_json::to_string(&lowered.footnotes)
            .map_err(|failure| napi::Error::from_reason(format!("footnote serialization failed: {failure}")))
    }
}

/// One downloaded math face registered under a resource package id.
struct InstalledMathFace {
    /// Hash the bytes were installed from, so a repeated render skips the leak.
    hash: String,
    /// Leaked name the native family registry keyed the payload on.
    family: &'static str,
    /// TeX parameter family the package declared, if any.
    tex_parameters: Option<&'static str>,
}

fn installed_math_faces() -> &'static Mutex<BTreeMap<String, InstalledMathFace>> {
    static FACES: OnceLock<Mutex<BTreeMap<String, InstalledMathFace>>> = OnceLock::new();
    FACES.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// TeX parameter families a math-font package may declare in `math-font.json`.
///
/// The list is the native contract, not a TypeScript copy: the package reader
/// validates the declared label against this answer before installing a face.
#[napi]
pub fn math_font_tex_parameter_families() -> Vec<String> {
    vutex_compose::math::MathSemantics::TEX_PARAMETER_FAMILIES.iter().map(|label| (*label).to_string()).collect()
}

/// Register a downloaded OpenType MATH face under its resource package id.
///
/// Resource packages that ship a face the crate did not compile in call this
/// before `build_display`, so the name the document declared resolves to the
/// bytes the package downloaded. The face must carry a parsable `MATH` table;
/// anything else fails here instead of falling back to synthetic metrics. A
/// package may additionally declare the TeX parameter family its metrics
/// follow, which is how a downloaded face inherits the σ16 lone-subscript
/// correction; an unknown family is rejected rather than silently ignored. A
/// repeat install with the same artifact hash and declaration is a no-op, so a
/// long-running host does not leak one copy of the payload per render.
#[napi]
pub fn install_math_font_face(
    name: String,
    artifact_hash: String,
    bytes: Buffer,
    tex_parameters: Option<String>,
) -> napi::Result<()> {
    install_embedded_math_fonts();
    let id = name.trim();
    if id.is_empty() {
        return Err(napi::Error::from_reason("math font package id is empty"));
    }
    if assets::math_family(id).is_some() {
        return Err(napi::Error::from_reason(format!("math font package id is reserved for a bundled face: {id}")));
    }
    let declared = tex_parameters.as_deref().map(str::trim).filter(|label| !label.is_empty());
    if let Some(label) = declared {
        if !vutex_compose::math::MathSemantics::tex_parameter_family_known(label) {
            return Err(napi::Error::from_reason(format!(
                "math font package {id} declares unknown TeX parameter family: {label}"
            )));
        }
    }
    let payload = bytes.as_ref();
    if payload.is_empty() {
        return Err(napi::Error::from_reason(format!("math font package {id} carries no face bytes")));
    }
    let mut faces = installed_math_faces().lock().map_err(|_| napi::Error::from_reason("math face registry is poisoned"))?;
    if faces
        .get(id)
        .is_some_and(|face| face.hash == artifact_hash && face.tex_parameters == declared)
    {
        return Ok(());
    }
    if vutex_text::math::MathFontFacts::from_bytes(payload).is_none() {
        return Err(napi::Error::from_reason(format!(
            "math font package {id} has no parsable OpenType MATH table"
        )));
    }
    let family: &'static str = Box::leak(id.to_string().into_boxed_str());
    let leaked: &'static [u8] = Box::leak(payload.to_vec().into_boxed_slice());
    let parameters: Option<&'static str> = declared.map(|label| &*Box::leak(label.to_string().into_boxed_str()));
    match parameters {
        Some(label) => vutex_text::math::install_math_font_named_with_parameters(family, leaked, label),
        None => vutex_text::math::install_math_font_named(family, leaked),
    }
    faces.insert(id.to_string(), InstalledMathFace { hash: artifact_hash, family, tex_parameters: parameters });
    Ok(())
}

/// Resolve a declared math package to the family it installs.
///
/// The package id is the one the lockfile activated. Bundled faces resolve
/// through the asset pack; downloaded faces resolve through the registry built
/// by [`install_math_font_face`]. An id outside both, or a face the process
/// never installed, is a host error rather than a silent fallback: a document
/// that declared a math face must not paint another one.
fn resolve_math_font(math_font: Option<String>) -> napi::Result<Option<MathFontFamily>> {
    let Some(id) = math_font.as_deref().map(str::trim).filter(|id| !id.is_empty()) else {
        return Ok(None);
    };
    if let Some(family) = assets::math_family(id) {
        if family.bytes().is_none() {
            return Err(napi::Error::from_reason(format!("math font package is not installed: {id}")));
        }
        return Ok(Some(family));
    }
    let custom = installed_math_faces()
        .lock()
        .map_err(|_| napi::Error::from_reason("math face registry is poisoned"))?
        .get(id)
        .map(|face| face.family);
    match custom {
        Some(name) => Ok(Some(MathFontFamily::Custom(name))),
        None => Err(napi::Error::from_reason(format!("unknown math font package: {id}"))),
    }
}

/// Page geometry a template package contributes to the layout pass.
#[napi(object)]
pub struct TemplateGeometryInput {
    /// Page width in PostScript points.
    pub page_width: f64,
    /// Page height in PostScript points.
    pub page_height: f64,
    /// Top margin in PostScript points.
    pub margin_top: Option<f64>,
    /// Right margin in PostScript points.
    pub margin_right: Option<f64>,
    /// Bottom margin in PostScript points.
    pub margin_bottom: Option<f64>,
    /// Left margin in PostScript points.
    pub margin_left: Option<f64>,
    /// Header source: `title`, `none`, or literal text.
    pub chrome_header: Option<String>,
    /// Whether pages carry a folio.
    pub chrome_folio: Option<bool>,
    /// 1-based page index where chrome starts.
    pub chrome_start_page: Option<u32>,
    /// Running header alignment: `left`, `center`, or `right`.
    pub header_align: Option<String>,
    /// Heading levels that start a new page.
    pub page_break_before: Option<Vec<u32>>,
    /// Body text size in points.
    pub body_size: Option<f64>,
    /// Line box as a multiple of the body size.
    pub line_height: Option<f64>,
    /// Heading size as a multiple of the body size, H1 to H4.
    pub heading_scales: Option<Vec<f64>>,
}

/// Paginate a lowered document under the selected theme and MATH family.
#[napi]
pub fn build_display(
    lowered: &LoweredDocumentHandle,
    theme: Option<String>,
    math_font: Option<String>,
    template: Option<TemplateGeometryInput>,
) -> napi::Result<DisplaySceneHandle> {
    install_embedded_math_fonts();
    let selected = Theme::from_option(theme.as_deref());
    let family = resolve_math_font(math_font)?;
    let geometry = match template {
        Some(input) => Some(template_geometry(input)?),
        None => None,
    };
    let inner = lowered
        .inner
        .lock()
        .map_err(|_| napi::Error::from_reason("lowered document is poisoned"))?
        .take()
        .ok_or_else(|| napi::Error::from_reason("lowered document was consumed"))?;
    Ok(DisplaySceneHandle { inner: document::build_display(inner, selected, family, geometry) })
}

/// Validate and translate the JS template payload into the layout contract.
fn template_geometry(input: TemplateGeometryInput) -> napi::Result<document::TemplateGeometry> {
    let chrome_header = match input.chrome_header.as_deref() {
        None => None,
        Some("title") => Some("title"),
        Some("none") => Some("none"),
        Some(text) => Some(intern_template_text(text.to_string())),
    };
    let header_align = match input.header_align.as_deref() {
        None => None,
        Some("left") => Some(Align::Left),
        Some("center") => Some(Align::Center),
        Some("right") => Some(Align::Right),
        Some(other) => return Err(napi::Error::from_reason(format!("unknown template header alignment: {other}"))),
    };
    let mut section_breaks = [false; 4];
    for level in input.page_break_before.unwrap_or_default() {
        if !(1..=4).contains(&level) {
            return Err(napi::Error::from_reason(format!("template page break level is outside H1 to H4: {level}")));
        }
        section_breaks[(level - 1) as usize] = true;
    }
    let heading_scales = match input.heading_scales {
        None => None,
        Some(values) => {
            let scales: [f64; 4] = values
                .try_into()
                .map_err(|_| napi::Error::from_reason("template heading scales need exactly four values"))?;
            Some(scales)
        }
    };
    Ok(document::TemplateGeometry {
        page_width: input.page_width,
        page_height: input.page_height,
        margin_top: input.margin_top,
        margin_right: input.margin_right,
        margin_bottom: input.margin_bottom,
        margin_left: input.margin_left,
        chrome_header,
        chrome_folio: input.chrome_folio,
        chrome_start_page: input.chrome_start_page.map(|page| page as usize),
        header_align,
        section_breaks,
        body_size: input.body_size,
        line_height: input.line_height,
        heading_scales,
    })
}

/// Paginated display scene exposed to the host.
#[napi]
pub struct DisplaySceneHandle {
    inner: document::DisplayScene,
}

#[napi]
impl DisplaySceneHandle {
    /// Number of pages.
    #[napi]
    pub fn page_count(&self) -> u32 {
        self.inner.display.pages.len() as u32
    }

    /// Number of placed items across pages.
    #[napi]
    pub fn item_count(&self) -> u32 {
        self.inner.display.pages.iter().map(|page| page.items.len()).sum::<usize>() as u32
    }

    /// Theme id the scene paints with.
    #[napi]
    pub fn theme_id(&self) -> String {
        self.inner.theme.id().to_string()
    }

    /// MATH family id the scene laid formulas out with, when one was declared.
    #[napi]
    pub fn math_font_id(&self) -> Option<String> {
        self.inner.math_font.map(str::to_string)
    }

    /// Serialized layout diagnostics.
    #[napi]
    pub fn diagnostics_json(&self) -> String {
        serde_json::to_string(&self.inner.diagnostics).unwrap_or_else(|_| "[]".to_string())
    }

    /// Emit SVG bytes with the bundled fonts embedded.
    #[napi]
    pub fn svg(&self) -> napi::Result<String> {
        document::render_display_svg(&self.inner).map_err(|record| napi::Error::from_reason(record.message))
    }

    /// Emit fixed-geometry snapshot HTML with the bundled fonts embedded.
    #[napi]
    pub fn snapshot_html(&self) -> napi::Result<String> {
        document::render_display_snapshot_html(&self.inner).map_err(|record| napi::Error::from_reason(record.message))
    }

    /// Emit PDF bytes with bundled faces and caller-supplied image resources.
    #[napi]
    pub fn pdf(&self, resources_json: Option<String>) -> napi::Result<PdfRenderResponse> {
        let outcome = document::render_display_pdf(&self.inner, resources_json.as_deref())
            .map_err(|record| napi::Error::from_reason(record.message))?;
        Ok(PdfRenderResponse {
            pdf: Buffer::from(outcome.pdf),
            diagnostics_json: serde_json::to_string(&outcome.diagnostics).unwrap_or_else(|_| "[]".to_string()),
        })
    }
}

/// Build the semantic reflow scene for HTML output.
#[napi]
pub fn build_flow(graph_json: String, theme: Option<String>) -> napi::Result<FlowSceneHandle> {
    let graph: DocumentGraph = serde_json::from_str(&graph_json)
        .map_err(|failure| napi::Error::from_reason(format!("document graph is not valid JSON: {failure}")))?;
    let selected = Theme::from_option(theme.as_deref());
    Ok(FlowSceneHandle { inner: document::build_flow(&graph, selected) })
}

/// Semantic reflow scene exposed to the host.
#[napi]
pub struct FlowSceneHandle {
    inner: document::FlowScene,
}

#[napi]
impl FlowSceneHandle {
    /// Emit reflow HTML with the bundled fonts embedded.
    #[napi]
    pub fn html(&self) -> napi::Result<String> {
        document::render_flow_html(&self.inner).map_err(|record| napi::Error::from_reason(record.message))
    }
}

/// Embed bundled fonts into an SVG payload a host already holds.
#[napi]
pub fn embed_svg_fonts(svg: String, theme: Option<String>) -> String {
    let selected = Theme::from_option(theme.as_deref());
    document::embed_svg_fonts(&svg, selected)
}

/// Subset one bundled face to an explicit character set.
///
/// Returns the subset program, or `null` when the face maps none of the
/// characters. `family` is `serif` or `mono`, `script` is `latin` or `cjk`,
/// `style` is `regular`, `bold`, or `italic`.
#[napi]
pub fn subset_face(family: String, script: String, style: String, chars: String) -> Option<Buffer> {
    let face = find_face(&family, &script, &style)?;
    let set: BTreeSet<char> = chars.chars().collect();
    match font_subset::subset_font(&face.bytes, &set)? {
        font_subset::FacePayload::Subset(subset) => Some(Buffer::from(subset.bytes)),
        font_subset::FacePayload::Full => Some(Buffer::from(face.bytes.to_vec())),
    }
}

fn find_face(family: &str, script: &str, style: &str) -> Option<&'static BodyFace> {
    let family = match family.trim().to_ascii_lowercase().as_str() {
        "mono" | "monospace" => BodyFamily::Mono,
        _ => BodyFamily::Serif,
    };
    let script = match script.trim().to_ascii_lowercase().as_str() {
        "cjk" | "han" => BodyScript::Cjk,
        _ => BodyScript::Latin,
    };
    let style = match style.trim().to_ascii_lowercase().as_str() {
        "bold" => BodyFaceStyle::Bold,
        "italic" => BodyFaceStyle::Italic,
        _ => BodyFaceStyle::Regular,
    };
    assets::body_faces()
        .iter()
        .find(|face| face.family == family && face.script == script && face.style == style)
}

/// Structural block counts for one serialized document graph.
#[napi]
pub fn summarize_document(graph_json: String) -> napi::Result<String> {
    let graph: DocumentGraph = serde_json::from_str(&graph_json)
        .map_err(|failure| napi::Error::from_reason(format!("document graph is not valid JSON: {failure}")))?;
    Ok(document::summarize_document(&graph).to_string())
}

/// Assemble a report from caller-supplied sections.
#[napi]
pub fn assemble_report(
    operation: String,
    inputs_json: String,
    diagnostics_json: String,
    output_json: Option<String>,
    extra_json: Option<String>,
) -> String {
    document::assemble_report(
        &operation,
        &inputs_json,
        &diagnostics_json,
        output_json.as_deref(),
        extra_json.as_deref(),
    )
}

/// Typography facts for the selected theme.
#[napi]
pub fn theme_facts(theme: Option<String>) -> String {
    serde_json::to_string(&document::theme_facts(Theme::from_option(theme.as_deref())))
        .unwrap_or_else(|_| "{}".to_string())
}
