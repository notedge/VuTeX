//! PDF output: a minimal, dependency-free PDF 1.7 writer.
//!
//! The writer paints the same display primitives as the SVG backend.
//! WinAnsi-representable text uses the base-14 standard fonts, so Latin
//! documents embed no font program. Text the WinAnsi encoding cannot reach is
//! painted with a subset of a discovered system TrueType font, embedded as an
//! Identity-H `CIDFontType2` program, so every character reaches the page.
//! Coordinates arrive with the origin at the page top-left corner and are
//! flipped to PDF's bottom-left origin by subtracting them from the page height.
//!
//! Image slots are raster XObjects when their source resolves to a PNG or
//! JPEG. An SVG source instead paints as vector geometry from a parsed scene,
//! so an icon keeps its curves at any resolution and registers no `/XObject`.
//! Sources resolve through a caller-owned [`PdfResources`] context, so a
//! page-level image slot and a scene image node share one decoder while the
//! host decides where the bytes come from. A raster object is shared by every
//! slot that names the same source, and an SVG is parsed once for the whole
//! document. The raster picture paints upright with a `q … cm /ImN Do Q` block
//! whose matrix reverses the image rows inside the y-flipped page. An absent,
//! unresolvable, or unsupported source keeps the dashed placeholder.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    path::PathBuf,
};

use davinci_scene::DisplayScene;
use vutex_presentation::display::{
    DisplayGraphic, DisplayImage, DisplayItem, DisplayPage, DisplayPresentation, DisplayRect, DisplayRule, DisplayTextRun,
};
use vutex_render::{RenderBackend, is_safe_href};
use vutex_types::{Diagnostic, Severity, math::FontStyle};

use crate::{
    font::{Subset, SystemFont},
    resources::PdfResources,
};

/// Base-14 fonts this backend can emit, in canonical resource order.
///
/// The table covers the three standard families in their regular, bold,
/// italic, and bold-italic faces, so `/F1../Fn` stays stable no matter which
/// styles a document uses.
const BASE_FONTS: [&str; 12] = [
    "Courier",
    "Courier-Bold",
    "Courier-BoldOblique",
    "Courier-Oblique",
    "Helvetica",
    "Helvetica-Bold",
    "Helvetica-BoldOblique",
    "Helvetica-Oblique",
    "Times-Roman",
    "Times-Bold",
    "Times-BoldItalic",
    "Times-Italic",
];

/// Run extent above the baseline, as a fraction of the font size.
///
/// This mirrors the layout contract that places every baseline at
/// `line top + 0.8 * size`, so a link rectangle covers the whole line box.
const ASCENT: f64 = 0.8;
/// Run extent below the baseline, as a fraction of the font size.
///
/// The base-14 faces have no embedded metrics here, so the descent is the
/// stacked complement of [`ASCENT`] rather than a per-font value.
const DESCENDER: f64 = 0.2;

/// PDF renderer. Consumes only the display presentation IR.
///
/// Fonts for text outside WinAnsi are discovered on the system; use
/// [`render_pdf_with_font_policy`] to pin or disable that lookup.
pub fn render_pdf(presentation: &DisplayPresentation) -> Result<Vec<u8>, Diagnostic> {
    render_pdf_with_font_policy(presentation, &PdfFontPolicy::Discover)
}

/// Where glyphs for text outside WinAnsi come from.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum PdfFontPolicy {
    /// Discover a system TrueType font when a document needs one.
    #[default]
    Discover,
    /// Never embed a font; characters outside WinAnsi degrade to `?`.
    Base14Only,
    /// Embed subsets loaded from this font file.
    File(PathBuf),
}

/// PDF renderer with an explicit font policy.
pub fn render_pdf_with_font_policy(presentation: &DisplayPresentation, policy: &PdfFontPolicy) -> Result<Vec<u8>, Diagnostic> {
    render_pdf_with_resources(presentation, policy, &PdfResources::default())
}

/// PDF renderer with an explicit font policy and caller-owned image resources.
///
/// Sources resolve through `resources`, so a host can point relative paths at a
/// base directory or hand over bytes it already fetched. The default context
/// reproduces the behaviour of [`render_pdf`] and [`render_pdf_with_font_policy`].
pub fn render_pdf_with_resources(
    presentation: &DisplayPresentation,
    policy: &PdfFontPolicy,
    resources: &PdfResources,
) -> Result<Vec<u8>, Diagnostic> {
    let backend = PdfBackend;
    backend.render_display_with(presentation, policy, resources)
}

struct PdfBackend;

impl PdfBackend {
    fn render_display_with(
        &self,
        presentation: &DisplayPresentation,
        policy: &PdfFontPolicy,
        resources: &PdfResources,
    ) -> Result<Vec<u8>, Diagnostic> {
        render_document(presentation, policy, resources)
    }
}

impl RenderBackend for PdfBackend {
    fn render_display(&self, presentation: &DisplayPresentation) -> Result<Vec<u8>, Diagnostic> {
        render_document(presentation, &PdfFontPolicy::Discover, &PdfResources::default())
    }
}

fn render_document(
    presentation: &DisplayPresentation,
    policy: &PdfFontPolicy,
    resources: &PdfResources,
) -> Result<Vec<u8>, Diagnostic> {
    if presentation.pages.is_empty() {
        return Err(Diagnostic {
            code: "vutex.render.pdf.no_pages".into(),
            severity: Severity::Error,
            message: "PDF output needs at least one display page with page geometry".into(),
        });
    }
    let objects = build_objects(presentation, policy, resources)?;
    Ok(assemble(&objects))
}

/// Whether a character has a byte in the WinAnsi encoding this writer emits.
fn representable(ch: char) -> bool {
    matches!(ch, ' '..='~') || (ch as u32) <= 0xFF || winansi_byte(ch).is_some()
}

/// Whether a text run needs glyphs beyond the base-14 fonts.
pub(crate) fn needs_embedding(text: &str) -> bool {
    text.chars().any(|ch| !representable(ch))
}

/// Base-14 face a scene text style asks for.
pub(crate) fn scene_font_style(weight: Option<u16>) -> FontStyle {
    match weight {
        Some(weight) if weight >= 600 => FontStyle::Bold,
        _ => FontStyle::Normal,
    }
}

/// Characters painted with the embedded font, in code point order.
fn embedded_charset(presentation: &DisplayPresentation) -> BTreeSet<char> {
    let mut charset = BTreeSet::new();
    for page in &presentation.pages {
        for item in &page.items {
            match item {
                DisplayItem::TextRun(run) => {
                    if needs_embedding(&run.text) {
                        charset.extend(run.text.chars());
                    }
                }
                DisplayItem::Graphic(graphic) => {
                    crate::scene::visit_texts(&graphic.scene, &mut |_, text| {
                        if needs_embedding(text) {
                            charset.extend(text.chars());
                        }
                    });
                }
                _ => {}
            }
        }
    }
    charset
}

/// Load and subset the font a document needs, when its policy allows it.
///
/// `Base14Only` deliberately keeps its degraded `?` output and `Ok(None)`. Any
/// other policy that cannot provide a glyph for every character returns a
/// diagnostic instead of silently dropping text or substituting `?`.
fn load_subset(presentation: &DisplayPresentation, policy: &PdfFontPolicy) -> Result<Option<Subset>, Diagnostic> {
    let charset = embedded_charset(presentation);
    if charset.is_empty() {
        return Ok(None);
    }
    if matches!(policy, PdfFontPolicy::Base14Only) {
        return Ok(None);
    }
    let font = match policy {
        PdfFontPolicy::Discover => SystemFont::discover(&charset).ok_or_else(|| Diagnostic {
            code: "vutex.render.pdf.font_unavailable".into(),
            severity: Severity::Error,
            message: "no system glyf TrueType font was found for the document charset".into(),
        })?,
        PdfFontPolicy::File(path) => SystemFont::load(path).ok_or_else(|| Diagnostic {
            code: "vutex.render.pdf.font_unavailable".into(),
            severity: Severity::Error,
            message: format!("font file {} is not a readable glyf TrueType font", path.display()),
        })?,
        PdfFontPolicy::Base14Only => return Ok(None),
    };
    let missing = font.missing(&charset);
    if !missing.is_empty() {
        return Err(Diagnostic {
            code: "vutex.render.pdf.glyph_missing".into(),
            severity: Severity::Error,
            message: format!("the embedded font has no glyph for {}", codepoints(&missing)),
        });
    }
    font.subset(&charset).map(Some).ok_or_else(|| Diagnostic {
        code: "vutex.render.pdf.glyph_missing".into(),
        severity: Severity::Error,
        message: "the embedded font subset produced no glyphs for the document charset".into(),
    })
}

/// Code points in `chars`, as `U+XXXX` tokens, for a diagnostic message.
fn codepoints(chars: &[char]) -> String {
    const LIMIT: usize = 16;
    let mut tokens: Vec<String> = chars.iter().take(LIMIT).map(|ch| format!("U+{:04X}", *ch as u32)).collect();
    if chars.len() > LIMIT {
        tokens.push(format!("and {} more", chars.len() - LIMIT));
    }
    tokens.join(" ")
}

/// Distinct scene alphas below full opacity, in ascending order.
///
/// The list drives one `/ExtGState` object per value, so scenes that share an
/// opacity share a single dictionary and resource name.
fn scene_opacities(presentation: &DisplayPresentation) -> Vec<f64> {
    let mut alphas: Vec<f64> = Vec::new();
    for page in &presentation.pages {
        for item in &page.items {
            let DisplayItem::Graphic(graphic) = item
            else {
                continue;
            };
            crate::scene::visit_opacities(&graphic.scene, &mut |alpha| {
                if !alphas.contains(&alpha) {
                    alphas.push(alpha);
                }
            });
        }
    }
    alphas.sort_by(f64::total_cmp);
    alphas
}

/// Base-14 font for a display font family and face style.
///
/// Families resolve by substring, so CSS stacks such as `sans-serif` keep
/// working; a style the base-14 set cannot express (math and plain sans/mono
/// labels) falls back to the family's regular face.
pub(crate) fn base_font_for(family: &str, style: FontStyle) -> &'static str {
    let lower = family.to_ascii_lowercase();
    if lower.contains("mono") {
        match style {
            FontStyle::Bold => "Courier-Bold",
            FontStyle::Italic => "Courier-Oblique",
            _ => "Courier",
        }
    }
    else if lower.contains("sans") {
        match style {
            FontStyle::Bold => "Helvetica-Bold",
            FontStyle::Italic => "Helvetica-Oblique",
            _ => "Helvetica",
        }
    }
    else {
        match style {
            FontStyle::Bold => "Times-Bold",
            FontStyle::Italic => "Times-Italic",
            _ => "Times-Roman",
        }
    }
}

/// Distinct base-14 fonts referenced by text runs, in canonical order.
///
/// Runs painted with the embedded font need no base-14 entry, but they do when
/// the policy disabled embedding or no system font was found.
fn used_fonts(presentation: &DisplayPresentation, embed_available: bool) -> Vec<&'static str> {
    let mut used: Vec<&'static str> = Vec::new();
    fn add(used: &mut Vec<&'static str>, base: &'static str) {
        if !used.contains(&base) {
            used.push(base);
        }
    }
    for page in &presentation.pages {
        for item in &page.items {
            match item {
                DisplayItem::TextRun(run) => {
                    if embed_available && needs_embedding(&run.text) {
                        continue;
                    }
                    add(&mut used, base_font_for(&run.font_family, run.style));
                }
                DisplayItem::Graphic(graphic) => {
                    crate::scene::visit_texts(&graphic.scene, &mut |style, text| {
                        if embed_available && needs_embedding(text) {
                            return;
                        }
                        let family = style.font_family.as_deref().unwrap_or("");
                        add(&mut used, base_font_for(family, scene_font_style(style.font_weight)));
                    });
                }
                _ => {}
            }
        }
    }
    BASE_FONTS.iter().copied().filter(|base| used.contains(base)).collect()
}

/// A link annotation produced by one safe text-run destination.
#[derive(Debug, Clone, PartialEq)]
struct LinkAnnotation {
    /// Lower-left x, in PDF user space.
    x1: f64,
    /// Lower-left y, in PDF user space.
    y1: f64,
    /// Upper-right x, in PDF user space.
    x2: f64,
    /// Upper-right y, in PDF user space.
    y2: f64,
    /// Destination written into the `/URI` action.
    uri: String,
    /// Hover text written into `/Contents`, when the run carried a title.
    title: Option<String>,
}

/// One indirect object: a dictionary whose body is text, or a stream with bytes.
#[derive(Debug)]
enum PdfObject {
    /// A dictionary or array serialized as ASCII.
    Text(String),
    /// A dictionary plus a `/Length`-prefixed binary stream body.
    Stream { dict: String, data: Vec<u8> },
}

impl PdfObject {
    /// Dictionary of a stream object, without its `/Length` entry.
    fn stream(dict: String, data: Vec<u8>) -> Self {
        Self::Stream { dict, data }
    }
}

/// Embedded font program shared by every page, plus its resource index.
#[derive(Debug, Clone, Copy)]
pub(crate) struct EmbeddedText<'a> {
    /// Subset carrying the `char` to CID mapping used by content streams.
    pub(crate) subset: &'a Subset,
    /// One-based `/F` resource index the content stream selects.
    pub(crate) resource_index: usize,
}

/// `/ExtGState` resources for the distinct scene alphas a document paints.
///
/// One object serves every page, so a scene value that recurs across pages
/// still costs a single dictionary and a single resource name.
#[derive(Debug, Default)]
pub(crate) struct OpacityResources {
    /// Resource name and alpha, in ascending alpha order.
    entries: Vec<(String, f64)>,
}

impl OpacityResources {
    /// Resource name selecting the graphics state for an alpha.
    ///
    /// The collector visits the same scene tree with the same arithmetic as the
    /// painter, so only exact matches occur and a missing name means the node
    /// paints fully opaque.
    pub(crate) fn name(&self, alpha: f64) -> Option<&str> {
        self.entries.iter().find(|(_, value)| *value == alpha).map(|(name, _)| name.as_str())
    }
}

/// Image sources resolved once for the whole document.
///
/// A raster source maps to its `/ImN` resource name, while an SVG source keeps
/// the parsed scene so the content stream can paint vector geometry. Both
/// tables are keyed by the exact source string, so a source used twice is
/// decoded or parsed once.
#[derive(Debug, Default)]
pub(crate) struct ImageResources {
    /// `/ImN` resource name per raster source.
    rasters: BTreeMap<String, String>,
    /// Parsed scene per SVG source.
    scenes: BTreeMap<String, DisplayScene>,
}

impl ImageResources {
    /// `/ImN` name of a resolved raster, when the source was a PNG or JPEG.
    pub(crate) fn raster(&self, source: &str) -> Option<&str> {
        self.rasters.get(source).map(String::as_str)
    }

    /// Parsed scene of a resolved SVG source.
    pub(crate) fn scene(&self, source: &str) -> Option<&DisplayScene> {
        self.scenes.get(source)
    }

    /// Whether a source was already decoded or parsed.
    fn contains(&self, source: &str) -> bool {
        self.rasters.contains_key(source) || self.scenes.contains_key(source)
    }
}

/// Object streams in object-number order; index `i` is object `i + 1`.
fn build_objects(
    presentation: &DisplayPresentation,
    policy: &PdfFontPolicy,
    sources: &PdfResources,
) -> Result<Vec<PdfObject>, Diagnostic> {
    let subset = load_subset(presentation, policy)?;
    let fonts = used_fonts(presentation, subset.is_some());
    let page_count = presentation.pages.len();
    let page_links: Vec<Vec<LinkAnnotation>> = presentation.pages.iter().map(page_links).collect();

    let mut document = Document::default();
    document.push(PdfObject::Text(catalog_object()));
    document.push(PdfObject::Text(pages_object(page_count)));
    // Pages and their content come first so the page tree keeps object numbers
    // that do not shift when a document gains shared resources.
    let mut page_numbers = Vec::with_capacity(page_count);
    let mut content_numbers = Vec::with_capacity(page_count);
    for _ in 0..page_count {
        page_numbers.push(document.push(PdfObject::Text(String::new())));
        content_numbers.push(document.push(PdfObject::Text(String::new())));
    }

    let mut page_resources = PageResources::default();
    let mut font_objects: Vec<(&'static str, usize)> = Vec::with_capacity(fonts.len());
    for (index, base) in fonts.iter().enumerate() {
        let object = document.push(PdfObject::Text(font_object(base)));
        page_resources.insert("Font", &format!("F{}", index + 1), object);
        font_objects.push((*base, object));
    }
    let embedded_index = fonts.len() + 1;
    if let Some(subset) = &subset {
        let first = document.next_number();
        page_resources.insert("Font", &format!("F{embedded_index}"), first);
        push_embedded_font_objects(&mut document, subset, first);
    }
    let embedded = subset.as_ref().map(|subset| EmbeddedText { subset, resource_index: embedded_index });

    let mut opacities = OpacityResources::default();
    for (index, alpha) in scene_opacities(presentation).into_iter().enumerate() {
        let name = format!("GS{}", index + 1);
        let object = document.push(PdfObject::Text(ext_gstate_object(alpha)));
        page_resources.insert("ExtGState", &name, object);
        opacities.entries.push((name, alpha));
    }

    let annotation_numbers = push_link_annotations(&mut document, &page_links);
    let images = register_images(presentation, &mut document, sources, &mut page_resources);

    for (index, page) in presentation.pages.iter().enumerate() {
        let content = page_content(page, &font_objects, embedded, &opacities, &images);
        document.replace(content_numbers[index], content_object(&content));
        document.replace(
            page_numbers[index],
            PdfObject::Text(page_object(page, &page_resources.dict(), content_numbers[index], &annotation_numbers[index])),
        );
    }
    Ok(document.finish())
}

/// Resolve every image one presentation references, once per distinct source.
///
/// Raster sources become `/ImN` XObjects and SVG sources become parsed scenes.
/// Both page-level image slots and scene image nodes contribute, so the whole
/// document shares one raster object per distinct source and parses each SVG
/// once. Sources resolve through the caller's [`PdfResources`]. A transparent
/// raster contributes a pair: its `/DeviceGray` soft mask is appended first and
/// the colour image that references it as `/SMask` receives the `/ImN` name. A
/// reference that cannot be resolved is left out, so painting falls back to the
/// dashed slot.
fn register_images(
    presentation: &DisplayPresentation,
    document: &mut Document,
    sources: &PdfResources,
    page_resources: &mut PageResources,
) -> ImageResources {
    let mut images = ImageResources::default();
    for page in &presentation.pages {
        for item in &page.items {
            match item {
                DisplayItem::Image(image) => {
                    if let Some(source) = &image.source {
                        register_image(source, None, document, sources, page_resources, &mut images);
                    }
                }
                DisplayItem::Graphic(graphic) => {
                    crate::scene::visit_images(&graphic.scene, &mut |resource| {
                        register_image(
                            &resource.href,
                            resource.mime.as_deref(),
                            document,
                            sources,
                            page_resources,
                            &mut images,
                        );
                    });
                }
                _ => {}
            }
        }
    }
    images
}

/// Resolve one source unless an earlier slot already handled it.
///
/// An SVG becomes vector geometry and registers nothing in `/XObject`; anything
/// else goes through the raster decoder, and a payload that is neither PNG nor
/// JPEG leaves the source unresolved.
fn register_image(
    source: &str,
    mime: Option<&str>,
    document: &mut Document,
    sources: &PdfResources,
    page_resources: &mut PageResources,
    images: &mut ImageResources,
) {
    if images.contains(source) {
        return;
    }
    if let Some(scene) = sources.load_svg(source, mime) {
        images.scenes.insert(source.to_string(), scene);
        return;
    }
    let Some(raster) = sources.load(source)
    else {
        return;
    };
    let smask = raster.smask().map(|(dict, alpha)| document.push(PdfObject::stream(dict, alpha.to_vec())));
    let object = document.push(PdfObject::stream(raster.dict(smask), raster.data));
    let name = format!("Im{}", images.rasters.len() + 1);
    page_resources.insert("XObject", &name, object);
    images.rasters.insert(source.to_string(), name);
}

/// Append every link annotation and report their numbers in paint order.
fn push_link_annotations(document: &mut Document, page_links: &[Vec<LinkAnnotation>]) -> Vec<Vec<usize>> {
    let mut numbers = Vec::with_capacity(page_links.len());
    for links in page_links {
        numbers.push(links.iter().map(|link| document.push(PdfObject::Text(annotation_object(link)))).collect());
    }
    numbers
}

/// Indirect objects in append order; object number `n` is `objects[n - 1]`.
#[derive(Debug, Default)]
struct Document {
    objects: Vec<PdfObject>,
}

impl Document {
    /// Append an object and return its one-based object number.
    fn push(&mut self, object: PdfObject) -> usize {
        self.objects.push(object);
        self.objects.len()
    }

    /// Number the next appended object receives.
    fn next_number(&self) -> usize {
        self.objects.len() + 1
    }

    /// Replace the object stored at a one-based object number.
    fn replace(&mut self, number: usize, object: PdfObject) {
        if let Some(slot) = self.objects.get_mut(number.saturating_sub(1)) {
            *slot = object;
        }
    }

    /// Objects in object-number order.
    fn finish(self) -> Vec<PdfObject> {
        self.objects
    }
}

/// One category of a page `/Resources` dictionary, such as `Font` or `ExtGState`.
#[derive(Debug)]
struct ResourceGroup {
    /// PDF resource category written as the dictionary key.
    category: &'static str,
    /// Resource name and object number, in insertion order.
    entries: Vec<(String, usize)>,
}

/// Named resources shared by every page.
#[derive(Debug, Default)]
struct PageResources {
    /// Resource categories in first-insertion order.
    groups: Vec<ResourceGroup>,
}

impl PageResources {
    /// Register one named resource under a category.
    ///
    /// `name` is the PDF name without its leading slash, so content streams can
    /// select it as `/Name`.
    fn insert(&mut self, category: &'static str, name: &str, object: usize) {
        if !self.groups.iter().any(|group| group.category == category) {
            self.groups.push(ResourceGroup { category, entries: Vec::new() });
        }
        if let Some(group) = self.groups.iter_mut().find(|group| group.category == category) {
            group.entries.push((name.to_string(), object));
        }
    }

    /// The `/Resources` dictionary body.
    fn dict(&self) -> String {
        let groups: Vec<String> = self
            .groups
            .iter()
            .filter(|group| !group.entries.is_empty())
            .map(|group| {
                let entries: Vec<String> = group.entries.iter().map(|(name, object)| format!("/{name} {object} 0 R")).collect();
                format!("/{} << {} >>", group.category, entries.join(" "))
            })
            .collect();
        if groups.is_empty() { "<< >>".to_string() } else { format!("<< {} >>", groups.join(" ")) }
    }
}

/// Append the five objects of an embedded font program, in object order.
///
/// The order is fixed by [`build_objects`]: Type0 font, CIDFont, font
/// descriptor, `FontFile2` stream, then the `ToUnicode` stream.
fn push_embedded_font_objects(document: &mut Document, subset: &Subset, first: usize) {
    document.push(PdfObject::Text(type0_font_object(EMBEDDED_NAME, first + 1, first + 4)));
    document.push(PdfObject::Text(cid_font_object(EMBEDDED_NAME, first + 2, subset)));
    document.push(PdfObject::Text(font_descriptor_object(EMBEDDED_NAME, first + 3, subset)));
    document.push(PdfObject::stream(format!("<< /Length1 {} >>", subset.data.len()), subset.data.clone()));
    document.push(PdfObject::stream(String::new(), to_unicode_cmap(subset)));
}

/// Base font name of every embedded subset.
///
/// The six-letter prefix marks the program as a subset, as the PDF spec asks.
const EMBEDDED_NAME: &str = "AAAAAA+VutexSystemFont";

/// Link annotations for every safe destination on one page, in paint order.
///
/// The rectangle is the run's glyph box: the baseline sits at `page_height -
/// y`, the top at `0.8 * size` above it, and the bottom at `0.2 * size` below
/// it. Boxes are clipped to the page MediaBox so a run that overflows the page
/// never produces an annotation outside it.
fn page_links(page: &DisplayPage) -> Vec<LinkAnnotation> {
    let mut links = Vec::new();
    for item in &page.items {
        let DisplayItem::TextRun(run) = item
        else {
            continue;
        };
        let Some(uri) = run.href.as_deref().filter(|href| is_safe_href(href))
        else {
            continue;
        };
        if !all_finite(&[run.x, run.y, run.size, run.width]) {
            continue;
        }
        let x1 = run.x.clamp(0.0, page.width);
        let x2 = (run.x + run.width).clamp(0.0, page.width);
        let y1 = (page.height - (run.y + DESCENDER * run.size)).clamp(0.0, page.height);
        let y2 = (page.height - (run.y - ASCENT * run.size)).clamp(0.0, page.height);
        if x2 <= x1 || y2 <= y1 {
            continue;
        }
        links.push(LinkAnnotation { x1, y1, x2, y2, uri: uri.to_string(), title: run.title.clone() });
    }
    links
}

fn catalog_object() -> String {
    "<< /Type /Catalog /Pages 2 0 R >>".to_string()
}

fn pages_object(page_count: usize) -> String {
    let kids: Vec<String> = (0..page_count).map(|index| format!("{} 0 R", 3 + 2 * index)).collect();
    format!("<< /Type /Pages /Kids [{}] /Count {page_count} >>", kids.join(" "))
}

fn page_object(page: &DisplayPage, resources: &str, content_number: usize, annotations: &[usize]) -> String {
    let mut object = format!(
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Resources {resources} /Contents {content_number} 0 R",
        num(page.width),
        num(page.height),
    );
    if !annotations.is_empty() {
        let references: Vec<String> = annotations.iter().map(|number| format!("{number} 0 R")).collect();
        let _ = write!(object, " /Annots [{}]", references.join(" "));
    }
    object.push_str(" >>");
    object
}

/// Link annotation: a URI action over the run's glyph rectangle, carrying the
/// authored hover text as `/Contents` when the run had one.
fn annotation_object(link: &LinkAnnotation) -> String {
    let mut object = format!(
        "<< /Type /Annot /Subtype /Link /Rect [{} {} {} {}] /Border [0 0 0] /A << /S /URI /URI ({}) >>",
        num(link.x1),
        num(link.y1),
        num(link.x2),
        num(link.y2),
        // Same literal-string escaping as text, so a destination containing
        // parentheses or backslashes cannot break the dictionary syntax.
        escape_text(&link.uri),
    );
    if let Some(title) = &link.title {
        let _ = write!(object, " /Contents ({})", escape_text(title));
    }
    object.push_str(" >>");
    object
}

fn font_object(base: &str) -> String {
    format!("<< /Type /Font /Subtype /Type1 /BaseFont /{base} /Encoding /WinAnsiEncoding >>")
}

/// Type0 font that selects the embedded CID program with `Identity-H`.
///
/// With an identity CMap the CID of a glyph equals its glyph id, so the
/// content stream can address subset glyphs by number.
fn type0_font_object(name: &str, cid_object: usize, to_unicode_object: usize) -> String {
    format!(
        "<< /Type /Font /Subtype /Type0 /BaseFont /{name} /Encoding /Identity-H \
         /DescendantFonts [{cid_object} 0 R] /ToUnicode {to_unicode_object} 0 R >>"
    )
}

fn cid_font_object(name: &str, descriptor_object: usize, subset: &Subset) -> String {
    let widths: Vec<String> = subset.widths.iter().map(|(cid, width)| format!("{cid} [{width}]")).collect();
    format!(
        "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /{name} \
         /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> \
         /FontDescriptor {descriptor_object} 0 R /CIDToGIDMap /Identity /DW 1000 /W [{}] >>",
        widths.join(" ")
    )
}

fn font_descriptor_object(name: &str, font_file_object: usize, subset: &Subset) -> String {
    format!(
        "<< /Type /FontDescriptor /FontName /{name} /Flags 4 \
         /FontBBox [{} {} {} {}] /ItalicAngle {} /Ascent {} /Descent {} /CapHeight {} /StemV 80 \
         /FontFile2 {font_file_object} 0 R >>",
        subset.bbox[0],
        subset.bbox[1],
        subset.bbox[2],
        subset.bbox[3],
        num(subset.italic_angle),
        subset.ascent,
        subset.descent,
        subset.cap_height,
    )
}

/// `ToUnicode` CMap mapping subset CIDs back to the code points they paint.
///
/// The mapping keeps copied text and text extraction readable even though the
/// content stream addresses glyph ids rather than characters.
fn to_unicode_cmap(subset: &Subset) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(
        b"/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n\
          /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n\
          /CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n\
          1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
    );
    let entries: Vec<(&char, &u16)> = subset.cids.iter().collect();
    for chunk in entries.chunks(100) {
        out.extend_from_slice(format!("{} beginbfchar\n", chunk.len()).as_bytes());
        for (ch, cid) in chunk {
            let code = **ch as u32;
            out.extend_from_slice(format!("<{cid:04X}> <{code:04X}>\n").as_bytes());
        }
        out.extend_from_slice(b"endbfchar\n");
    }
    out.extend_from_slice(b"endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    out
}

fn content_object(content: &str) -> PdfObject {
    PdfObject::stream(format!("<< /Length {} >>", content.len()), content.as_bytes().to_vec())
}

/// Content stream operators for one page; always ends with a newline.
fn page_content(
    page: &DisplayPage,
    fonts: &[(&'static str, usize)],
    embedded: Option<EmbeddedText<'_>>,
    opacities: &OpacityResources,
    images: &ImageResources,
) -> String {
    let mut out = String::new();
    let painter = crate::scene::ScenePaint { fonts, embedded, images, opacities };
    for item in &page.items {
        write_item(&mut out, page.height, item, fonts, embedded, painter);
    }
    if out.is_empty() {
        out.push('\n');
    }
    out
}

fn write_item(
    out: &mut String,
    page_height: f64,
    item: &DisplayItem,
    fonts: &[(&'static str, usize)],
    embedded: Option<EmbeddedText<'_>>,
    painter: crate::scene::ScenePaint<'_>,
) {
    match item {
        DisplayItem::TextRun(run) => write_text(out, page_height, run, fonts, embedded),
        DisplayItem::Rule(rule) => write_rule(out, page_height, rule),
        DisplayItem::Rect(rect) => write_rect(out, page_height, rect),
        DisplayItem::Image(image) => write_image(out, page_height, image, painter),
        DisplayItem::Graphic(graphic) => write_graphic(out, page_height, graphic, painter),
    }
}

fn write_text(
    out: &mut String,
    page_height: f64,
    run: &DisplayTextRun,
    fonts: &[(&'static str, usize)],
    embedded: Option<EmbeddedText<'_>>,
) {
    if !all_finite(&[run.x, run.y, run.size]) {
        return;
    }
    let resource_index = if embedded.is_some() && needs_embedding(&run.text) {
        embedded.map(|embedded| embedded.resource_index)
    }
    else {
        let base = base_font_for(&run.font_family, run.style);
        fonts.iter().position(|(font, _)| *font == base).map(|index| index + 1)
    };
    let Some(resource_index) = resource_index
    else {
        return;
    };
    paint_text(out, run.x, page_height - run.y, run.size, resource_index, &run.text, embedded, false);
}

/// Emit one text object at a page-space position.
///
/// `flip` inverts the text matrix so glyphs stay upright inside a y-flipped
/// scene-to-page matrix.
pub(crate) fn paint_text(
    out: &mut String,
    x: f64,
    y: f64,
    size: f64,
    resource_index: usize,
    text: &str,
    embedded: Option<EmbeddedText<'_>>,
    flip: bool,
) {
    let down = if flip { -1.0 } else { 1.0 };
    if let Some(embedded) = embedded
        && needs_embedding(text)
    {
        let glyphs: String = text.chars().filter_map(|ch| embedded.subset.cid(ch)).map(|cid| format!("{cid:04X}")).collect();
        let _ =
            write!(out, "BT\n/F{resource_index} {} Tf\n1 0 0 {down} {} {} Tm\n<{glyphs}> Tj\nET\n", num(size), num(x), num(y),);
        return;
    }
    let _ = write!(
        out,
        "BT\n/F{resource_index} {} Tf\n1 0 0 {down} {} {} Tm\n({}) Tj\nET\n",
        num(size),
        num(x),
        num(y),
        escape_text(text),
    );
}

fn write_rule(out: &mut String, page_height: f64, rule: &DisplayRule) {
    if !all_finite(&[rule.x1, rule.y1, rule.x2, rule.y2, rule.thickness]) {
        return;
    }
    let _ = write!(
        out,
        "0.067 G\n{} w\n{} {} m\n{} {} l\nS\n",
        num(rule.thickness),
        num(rule.x1),
        num(page_height - rule.y1),
        num(rule.x2),
        num(page_height - rule.y2),
    );
}

fn write_rect(out: &mut String, page_height: f64, rect: &DisplayRect) {
    if !all_finite(&[rect.x, rect.y, rect.width, rect.height]) {
        return;
    }
    let y = page_height - (rect.y + rect.height);
    // The fill colour stays inside `q`/`Q` so later text keeps the default
    // black fill instead of inheriting the background grey.
    if rect.filled {
        let _ = write!(out, "q\n0.949 g\n{} {} {} {} re\nf\nQ\n", num(rect.x), num(y), num(rect.width), num(rect.height));
    }
    else {
        let _ = write!(out, "q\n0.6 G\n0.5 w\n{} {} {} {} re\nS\nQ\n", num(rect.x), num(y), num(rect.width), num(rect.height));
    }
}

/// Paint one page-level image, or its dashed slot when the source did not resolve.
///
/// A raster paints as `/ImN Do`. PDF image space puts the first sample row at
/// the top of the unit square, while page space counts y downward from the page
/// top. The `d` entry is therefore positive and the translation places the unit
/// square's bottom edge at `page_height - (image.y + image.height)`, so the
/// first row lands on the box's top edge and the picture stays upright.
///
/// An SVG paints through the scene painter instead. Its scene is uniformly
/// fitted and centred inside the slot, and the painter's root matrix flips the
/// scene's y-down coordinates into PDF's y-up page space, so the icon is right
/// way up without any per-slot row reversal.
fn write_image(out: &mut String, page_height: f64, image: &DisplayImage, painter: crate::scene::ScenePaint<'_>) {
    if !all_finite(&[image.x, image.y, image.width, image.height]) {
        return;
    }
    if image.width > 0.0
        && image.height > 0.0
        && let Some(source) = image.source.as_deref()
    {
        if let Some(name) = painter.images.raster(source) {
            let bottom = page_height - (image.y + image.height);
            let _ = write!(
                out,
                "q\n{} 0 0 {} {} {} cm\n/{name} Do\nQ\n",
                num(image.width),
                num(image.height),
                num(image.x),
                num(bottom)
            );
            return;
        }
        if let Some(scene) = painter.images.scene(source)
            && crate::scene::write_scene_slot(out, page_height, scene, image.x, image.y, image.width, image.height, painter)
        {
            return;
        }
    }
    write_image_placeholder(out, page_height, image);
}

/// Reserved dashed slot for an image this backend could not resolve.
fn write_image_placeholder(out: &mut String, page_height: f64, image: &DisplayImage) {
    let y = page_height - (image.y + image.height);
    let _ = write!(
        out,
        "0.6 G\n0.5 w\n[3 2] 0 d\n{} {} {} {} re\nS\n[] 0 d\n",
        num(image.x),
        num(y),
        num(image.width),
        num(image.height),
    );
}

fn write_graphic(out: &mut String, page_height: f64, graphic: &DisplayGraphic, painter: crate::scene::ScenePaint<'_>) {
    if !all_finite(&[graphic.x, graphic.y, graphic.width, graphic.height]) {
        return;
    }
    crate::scene::write_scene(out, page_height, graphic, painter);
}

/// Graphics state selecting one alpha for both fill and stroke.
///
/// `/AIS false` keeps the alpha from applying to a soft mask, and `/BM /Normal`
/// fixes the blend mode so every reader composites the node the same way.
fn ext_gstate_object(alpha: f64) -> String {
    format!("<< /Type /ExtGState /ca {} /CA {} /AIS false /BM /Normal >>", num(alpha), num(alpha))
}

fn all_finite(values: &[f64]) -> bool {
    values.iter().copied().all(f64::is_finite)
}

/// Escape a string for a PDF literal string and encode it for WinAnsi.
///
/// `\`, `(`, and `)` are backslash-escaped, printable ASCII passes through,
/// Latin-1 code points become three-digit octal escapes, code points that
/// WinAnsi reaches through its upper range (typographic punctuation, the
/// bullet, the euro sign) are remapped to their WinAnsi byte, and everything
/// else becomes `?` because the encoding cannot represent it.
pub(crate) fn escape_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '(' => out.push_str("\\("),
            ')' => out.push_str("\\)"),
            ' '..='~' => out.push(ch),
            ch if (ch as u32) <= 0xFF => {
                let _ = write!(out, "\\{:03o}", ch as u32);
            }
            ch => match winansi_byte(ch) {
                Some(byte) => {
                    let _ = write!(out, "\\{byte:03o}");
                }
                None => out.push('?'),
            },
        }
    }
    out
}

/// WinAnsi byte for a code point above Latin-1 that the encoding still covers.
///
/// WinAnsiEncoding keeps the Latin-1 positions but replaces the `0x80..=0x9F`
/// block with printable punctuation, so code points such as U+2022 BULLET
/// (the list marker the layout pass emits) have a byte of their own instead of
/// degrading to `?`.
fn winansi_byte(ch: char) -> Option<u8> {
    let byte = match ch {
        '\u{20AC}' => 0x80,
        '\u{201A}' => 0x82,
        '\u{0192}' => 0x83,
        '\u{201E}' => 0x84,
        '\u{2026}' => 0x85,
        '\u{2020}' => 0x86,
        '\u{2021}' => 0x87,
        '\u{02C6}' => 0x88,
        '\u{2030}' => 0x89,
        '\u{0160}' => 0x8A,
        '\u{2039}' => 0x8B,
        '\u{0152}' => 0x8C,
        '\u{017D}' => 0x8E,
        '\u{2018}' => 0x91,
        '\u{2019}' => 0x92,
        '\u{201C}' => 0x93,
        '\u{201D}' => 0x94,
        '\u{2022}' => 0x95,
        '\u{2013}' => 0x96,
        '\u{2014}' => 0x97,
        '\u{02DC}' => 0x98,
        '\u{2122}' => 0x99,
        '\u{0161}' => 0x9A,
        '\u{203A}' => 0x9B,
        '\u{0153}' => 0x9C,
        '\u{017E}' => 0x9E,
        '\u{0178}' => 0x9F,
        _ => return None,
    };
    Some(byte)
}

/// Stable decimal rendering: at most three decimals, no trailing zeros, no `-0`.
pub(crate) fn num(value: f64) -> String {
    let text = format!("{value:.3}");
    let trimmed = text.trim_end_matches('0').trim_end_matches('.');
    if trimmed.is_empty() || trimmed == "-0" { "0".to_string() } else { trimmed.to_string() }
}

/// Serialize objects into a complete PDF file with a byte-exact xref table.
fn assemble(objects: &[PdfObject]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n");
    let mut offsets = Vec::with_capacity(objects.len());
    for (index, object) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        match object {
            PdfObject::Text(body) => out.extend_from_slice(body.as_bytes()),
            PdfObject::Stream { dict, data } => {
                let dict = if dict.is_empty() { format!("<< /Length {} >>", data.len()) } else { dict.clone() };
                out.extend_from_slice(dict.as_bytes());
                out.extend_from_slice(b"\nstream\n");
                out.extend_from_slice(data);
                out.extend_from_slice(b"\nendstream");
            }
        }
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref_offset = out.len();
    let size = objects.len() + 1;
    out.extend_from_slice(format!("xref\n0 {size}\n").as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for offset in &offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n").as_bytes());
    out
}
