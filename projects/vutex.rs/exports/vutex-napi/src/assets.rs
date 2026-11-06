//! Font asset pack owned by VuTeX.
//!
//! `document.fonts` ships no bytes, so this crate embeds the faces it paints
//! with and hands them over through the injection entry points. The files live
//! in `projects/vutex.rs/fonts/`, next to their OFL and GUST licence texts.

use std::sync::{Arc, OnceLock};

use document_fonts::{BodyFace, BodyFaceSet, BodyFaceStyle, BodyFamily, BodyScript, BodyMetrics, MathFontPack};
use vutex_text::math::MathFontFamily;

const TINOS_REGULAR: &[u8] = include_bytes!("../../../fonts/Tinos-Regular.ttf");
const TINOS_BOLD: &[u8] = include_bytes!("../../../fonts/Tinos-Bold.ttf");
const TINOS_ITALIC: &[u8] = include_bytes!("../../../fonts/Tinos-Italic.ttf");
const NOTO_SERIF_SC_REGULAR: &[u8] = include_bytes!("../../../fonts/NotoSerifSC-Regular.ttf");
const NOTO_SERIF_SC_BOLD: &[u8] = include_bytes!("../../../fonts/NotoSerifSC-Bold.ttf");
const NOTO_SANS_MONO_REGULAR: &[u8] = include_bytes!("../../../fonts/NotoSansMono-Regular.ttf");
const LATIN_MODERN_MATH: &[u8] = include_bytes!("../../../fonts/LatinModernMath-Regular.otf");
const XITS_MATH: &[u8] = include_bytes!("../../../fonts/XITSMath-Regular.otf");

/// Body faces in the fallback order layout and painting both rely on.
fn body_set_ref() -> &'static Arc<BodyFaceSet> {
    static SET: OnceLock<Arc<BodyFaceSet>> = OnceLock::new();
    SET.get_or_init(|| {
        Arc::new(BodyFaceSet::new(vec![
            BodyFace::new(BodyFamily::Serif, BodyScript::Latin, BodyFaceStyle::Regular, TINOS_REGULAR),
            BodyFace::new(BodyFamily::Serif, BodyScript::Latin, BodyFaceStyle::Bold, TINOS_BOLD),
            BodyFace::new(BodyFamily::Serif, BodyScript::Latin, BodyFaceStyle::Italic, TINOS_ITALIC),
            BodyFace::new(BodyFamily::Serif, BodyScript::Cjk, BodyFaceStyle::Regular, NOTO_SERIF_SC_REGULAR),
            BodyFace::new(BodyFamily::Serif, BodyScript::Cjk, BodyFaceStyle::Bold, NOTO_SERIF_SC_BOLD),
            BodyFace::new(BodyFamily::Mono, BodyScript::Latin, BodyFaceStyle::Regular, NOTO_SANS_MONO_REGULAR),
        ]))
    })
}

/// Shared handle for [`PdfFontPolicy::Provided`](vutex_render_pdf::PdfFontPolicy).
pub(crate) fn body_set() -> Arc<BodyFaceSet> {
    body_set_ref().clone()
}

/// Faces in the pack's declaration order.
pub(crate) fn body_faces() -> &'static [BodyFace] {
    body_set_ref().faces()
}

/// Metrics of the pack, parsed once.
pub(crate) fn body_metrics() -> &'static BodyMetrics {
    static METRICS: OnceLock<BodyMetrics> = OnceLock::new();
    METRICS.get_or_init(|| BodyMetrics::from_set(body_set_ref()))
}

/// Math faces installed into `document_text` at startup.
pub(crate) fn math_pack() -> MathFontPack {
    MathFontPack::new(LATIN_MODERN_MATH, XITS_MATH)
}

/// Bundled math packages, each naming the OpenType MATH family it installs.
///
/// The ids match the built-in package registry, so a document that declares
/// `[packages] math-font = "xits-math"` selects the same face the registry
/// reports as activated.
pub(crate) const MATH_PACKAGE_FAMILIES: [(&str, MathFontFamily); 2] =
    [("latin-modern-math", MathFontFamily::LmMath), ("xits-math", MathFontFamily::XitsMath)];

/// Resolve a bundled math package id to its family.
pub(crate) fn math_family(package: &str) -> Option<MathFontFamily> {
    MATH_PACKAGE_FAMILIES
        .iter()
        .find(|(id, _)| *id == package)
        .map(|(_, family)| *family)
}
