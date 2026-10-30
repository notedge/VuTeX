//! Image resources owned by the caller of the PDF backend.
//!
//! A scene image node and a page-level image slot name their picture with a
//! source string rather than carrying pixels, so the backend must decide where
//! the bytes come from. [`PdfResources`] records the caller's own decisions and
//! is threaded from the host binding down to the writer.
//!
//! A source resolves in this order:
//!
//! 1. the caller-supplied table, keyed by the exact source string,
//! 2. a `data:` URI with a base64 payload,
//! 3. an absolute filesystem path,
//! 4. a path relative to the configured base directory,
//! 5. a path relative to the process working directory, kept only for
//!    backwards compatibility.
//!
//! Anything else resolves to nothing, including `http(s)` and `file:` URLs the
//! caller did not supply, so a reference the backend cannot read keeps its
//! dashed placeholder instead of failing the document.
//!
//! A source that names an SVG document is parsed into vector geometry instead
//! of a raster; see [`crate::svg`] for the supported subset.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use davinci_scene::DisplayScene;

use crate::{
    image::{self, Raster},
    svg,
};

/// Caller-owned image resource context for the PDF backend.
///
/// The default context has no base directory and no supplied payloads, so
/// sources resolve exactly as they did before the context existed: an inline
/// `data:` URI, an absolute path, or a path relative to the process working
/// directory.
///
/// A host that already fetched an asset supplies its bytes through
/// [`PdfResources::with_supplied`] and takes over resolution for that source.
/// A host that renders documents from another directory sets
/// [`PdfResources::with_base_dir`] so relative sources stay next to the
/// document instead of following the process.
#[derive(Debug, Clone, Default)]
pub struct PdfResources {
    /// Directory relative filesystem sources resolve against.
    base_dir: Option<PathBuf>,
    /// Already-fetched bytes keyed by the exact source string.
    supplied: BTreeMap<String, Vec<u8>>,
}

impl PdfResources {
    /// Empty context, equivalent to [`PdfResources::default`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the directory relative filesystem sources resolve against.
    ///
    /// An absolute source still wins over this directory, so a document may mix
    /// anchored and relative references.
    pub fn with_base_dir(mut self, base_dir: impl Into<PathBuf>) -> Self {
        self.base_dir = Some(base_dir.into());
        self
    }

    /// Supply already-fetched bytes for one source string.
    ///
    /// The key is compared verbatim against the source a scene node or a page
    /// slot carries, which lets an `https://…` URL a host fetched itself embed
    /// without this backend making a network request. A payload that is not a
    /// supported image format falls back to the dashed placeholder instead of
    /// failing the document, and the filesystem is never consulted for a
    /// supplied key. SVG payloads paint as vector geometry.
    pub fn with_supplied(mut self, source: impl Into<String>, bytes: impl Into<Vec<u8>>) -> Self {
        self.supplied.insert(source.into(), bytes.into());
        self
    }

    /// Raster behind one source, following the documented lookup order.
    ///
    /// A supplied payload is decoded directly, so a caller that pre-fetched a
    /// source keeps full control over what the document paints.
    pub(crate) fn load(&self, source: &str) -> Option<Raster> {
        image::decode(&self.read(source)?)
    }

    /// Parsed vector scene behind one SVG source, following the documented
    /// lookup order.
    ///
    /// A source only counts as SVG when its path ends in `.svg`, its `data:`
    /// URI declares `image/svg+xml`, or the caller supplied that MIME type.
    /// A document this parser cannot honour returns `None`, so the slot keeps
    /// its dashed placeholder.
    pub(crate) fn load_svg(&self, source: &str, mime: Option<&str>) -> Option<DisplayScene> {
        if !svg::is_svg_source(source, mime) {
            return None;
        }
        svg::parse(&self.read(source)?)
    }

    /// Raw bytes behind one source, before any format decision.
    fn read(&self, source: &str) -> Option<Vec<u8>> {
        let source = source.trim();
        if source.is_empty() {
            return None;
        }
        if let Some(bytes) = self.supplied.get(source) {
            return Some(bytes.clone());
        }
        if is_data_uri(source) {
            return image::bytes(source);
        }
        self.read_file(source)
    }

    /// Read a filesystem source, preferring absolute paths and the base dir.
    fn read_file(&self, source: &str) -> Option<Vec<u8>> {
        if Path::new(source).is_absolute() {
            return image::bytes(source);
        }
        if let Some(base_dir) = &self.base_dir {
            let joined = base_dir.join(source);
            if joined.is_file() {
                return image::bytes(&joined.to_string_lossy());
            }
        }
        // Historical fallback: paths were once read relative to the process
        // working directory, and callers that relied on that still work.
        image::bytes(source)
    }
}

/// Whether a source is an inline `data:` URI.
fn is_data_uri(source: &str) -> bool {
    source.get(..5).is_some_and(|prefix| prefix.eq_ignore_ascii_case("data:"))
}
