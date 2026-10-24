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

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use crate::image::{self, Raster};

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
    /// PNG or JPEG falls back to the dashed placeholder instead of failing the
    /// document, and the filesystem is never consulted for a supplied key.
    pub fn with_supplied(mut self, source: impl Into<String>, bytes: impl Into<Vec<u8>>) -> Self {
        self.supplied.insert(source.into(), bytes.into());
        self
    }

    /// Raster behind one source, following the documented lookup order.
    ///
    /// A supplied payload is decoded directly, so a caller that pre-fetched a
    /// source keeps full control over what the document paints.
    pub(crate) fn load(&self, source: &str) -> Option<Raster> {
        let source = source.trim();
        if source.is_empty() {
            return None;
        }
        if let Some(bytes) = self.supplied.get(source) {
            return image::decode(bytes);
        }
        if is_data_uri(source) {
            return image::load(source);
        }
        self.load_file(source)
    }

    /// Read a filesystem source, preferring absolute paths and the base dir.
    fn load_file(&self, source: &str) -> Option<Raster> {
        if Path::new(source).is_absolute() {
            return image::load(source);
        }
        if let Some(base_dir) = &self.base_dir {
            let joined = base_dir.join(source);
            if joined.is_file() {
                return image::load(&joined.to_string_lossy());
            }
        }
        // Historical fallback: paths were once read relative to the process
        // working directory, and callers that relied on that still work.
        image::load(source)
    }
}

/// Whether a source is an inline `data:` URI.
fn is_data_uri(source: &str) -> bool {
    source.get(..5).is_some_and(|prefix| prefix.eq_ignore_ascii_case("data:"))
}
