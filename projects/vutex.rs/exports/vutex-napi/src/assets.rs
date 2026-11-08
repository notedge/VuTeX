//! Font asset registry owned by VuTeX.
//!
//! `document.fonts` ships no bytes, so a host configures the faces once per
//! process and every later render reads that registry. Two first-class entry
//! points fill it: [`configure_from_bytes`] takes payloads the host already
//! read, [`configure_from_dir`] reads a directory the host names, including a
//! system font directory. Neither falls back to the other.

use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};

use document_fonts::{BodyFace, BodyFaceSet, BodyFaceStyle, BodyFamily, BodyMetrics, BodyScript, MathFontPack};
use napi::bindgen_prelude::Buffer;
use vutex_text::math::MathFontFamily;

/// One text face the registry knows how to build by role.
struct TextRoleSpec {
    /// Cross-repository logical name carried by `fonts.json`.
    role: &'static str,
    /// File name the production asset pack uses for this role.
    file: &'static str,
    /// Family the face serves.
    family: BodyFamily,
    /// Script the face covers.
    script: BodyScript,
    /// Style the face carries.
    style: BodyFaceStyle,
}

/// Text roles in the fallback order the layout and painting paths expect.
const TEXT_ROLES: [TextRoleSpec; 6] = [
    TextRoleSpec {
        role: "text-serif-latin-regular",
        file: "Tinos-Regular.ttf",
        family: BodyFamily::Serif,
        script: BodyScript::Latin,
        style: BodyFaceStyle::Regular,
    },
    TextRoleSpec {
        role: "text-serif-latin-bold",
        file: "Tinos-Bold.ttf",
        family: BodyFamily::Serif,
        script: BodyScript::Latin,
        style: BodyFaceStyle::Bold,
    },
    TextRoleSpec {
        role: "text-serif-latin-italic",
        file: "Tinos-Italic.ttf",
        family: BodyFamily::Serif,
        script: BodyScript::Latin,
        style: BodyFaceStyle::Italic,
    },
    TextRoleSpec {
        role: "text-serif-cjk-regular",
        file: "NotoSerifSC-Regular.ttf",
        family: BodyFamily::Serif,
        script: BodyScript::Cjk,
        style: BodyFaceStyle::Regular,
    },
    TextRoleSpec {
        role: "text-serif-cjk-bold",
        file: "NotoSerifSC-Bold.ttf",
        family: BodyFamily::Serif,
        script: BodyScript::Cjk,
        style: BodyFaceStyle::Bold,
    },
    TextRoleSpec {
        role: "text-mono-latin-regular",
        file: "NotoSansMono-Regular.ttf",
        family: BodyFamily::Mono,
        script: BodyScript::Latin,
        style: BodyFaceStyle::Regular,
    },
];

/// Math roles and the OpenType MATH family each installs.
const MATH_ROLES: [(&str, MathFontFamily); 2] =
    [("math-latin-modern", MathFontFamily::LmMath), ("math-xits", MathFontFamily::XitsMath)];

/// Failure while configuring font assets.
#[derive(Debug, Clone)]
pub(crate) enum FontAssetsError {
    /// No manifest was found, or its structure cannot be read.
    ManifestInvalid(String),
    /// A required face is missing from the configured source.
    FileMissing(String),
    /// A different source was configured earlier in this process.
    AlreadyConfigured(String),
}

impl FontAssetsError {
    /// Diagnostic code the host prints verbatim.
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::ManifestInvalid(_) => "VUTEX_FONT_ASSETS_MANIFEST_INVALID",
            Self::FileMissing(_) => "VUTEX_FONT_ASSETS_FILE_MISSING",
            Self::AlreadyConfigured(_) => "VUTEX_FONT_ASSETS_ALREADY_CONFIGURED",
        }
    }

    /// `code: detail` reason handed to Node-API.
    pub(crate) fn reason(&self) -> String {
        let detail = match self {
            Self::ManifestInvalid(detail) | Self::FileMissing(detail) | Self::AlreadyConfigured(detail) => detail,
        };
        format!("{}: {detail}", self.code())
    }
}

/// Font assets locked in for this process.
struct Configured {
    /// Identity of the source that filled the registry.
    source: String,
    /// Body faces in fallback order.
    set: Arc<BodyFaceSet>,
    /// Metrics parsed from the body faces.
    metrics: BodyMetrics,
}

/// Process-wide configured assets, empty until a host configures them.
static CONFIGURED: Mutex<Option<&'static Configured>> = Mutex::new(None);

/// Read the role-to-file pairs a manifest declares.
///
/// A manifest is either the production form
/// `{"schema":"vutex.font-assets/v1","files":[…]}` or the bare
/// `{"role":"file"}` mapping a host supplies for a directory whose file names
/// differ, such as a system font directory.
fn parse_manifest(raw: &str) -> Result<BTreeMap<String, String>, FontAssetsError> {
    let value: serde_json::Value = serde_json::from_str(raw)
        .map_err(|error| FontAssetsError::ManifestInvalid(format!("manifest is not valid JSON: {error}")))?;
    let object =
        value.as_object().ok_or_else(|| FontAssetsError::ManifestInvalid("manifest is not a JSON object".to_string()))?;
    let mut roles = BTreeMap::new();
    if let Some(files) = object.get("files") {
        let schema = object.get("schema").and_then(serde_json::Value::as_str);
        if schema != Some("vutex.font-assets/v1") {
            return Err(FontAssetsError::ManifestInvalid("manifest schema is not `vutex.font-assets/v1`".to_string()));
        }
        let entries =
            files.as_array().ok_or_else(|| FontAssetsError::ManifestInvalid("manifest `files` is not an array".to_string()))?;
        for entry in entries {
            let entry = entry
                .as_object()
                .ok_or_else(|| FontAssetsError::ManifestInvalid("manifest entry is not an object".to_string()))?;
            let name = entry.get("name").and_then(serde_json::Value::as_str).filter(|name| !name.is_empty());
            let role = entry.get("role").and_then(serde_json::Value::as_str).filter(|role| !role.is_empty());
            let (Some(name), Some(role)) = (name, role)
            else {
                return Err(FontAssetsError::ManifestInvalid(format!("manifest entry is missing `name` or `role`: {entry:?}")));
            };
            insert_role(&mut roles, role, name)?;
        }
    }
    else {
        for (role, name) in object {
            let name = name
                .as_str()
                .filter(|name| !name.is_empty())
                .ok_or_else(|| FontAssetsError::ManifestInvalid(format!("role `{role}` does not name a file")))?;
            insert_role(&mut roles, role, name)?;
        }
    }
    Ok(roles)
}

/// Record one role, rejecting a manifest that names it twice.
fn insert_role(roles: &mut BTreeMap<String, String>, role: &str, name: &str) -> Result<(), FontAssetsError> {
    if roles.insert(role.to_string(), name.to_string()).is_some() {
        return Err(FontAssetsError::ManifestInvalid(format!("manifest declares role `{role}` twice")));
    }
    Ok(())
}

/// Resolve one role to its file name, naming the expected file when absent.
fn file_for(roles: &BTreeMap<String, String>, role: &str, expected: &str) -> Result<String, FontAssetsError> {
    roles
        .get(role)
        .cloned()
        .ok_or_else(|| FontAssetsError::FileMissing(format!("missing role `{role}` (expected file `{expected}`)")))
}

/// Build the registry from resolved payloads.
///
/// Every text role must be present, in the order layout expects, and both math
/// roles must be present before the process-wide math install runs.
fn assemble(
    source: String,
    roles: &BTreeMap<String, String>,
    mut read: impl FnMut(&str, &str) -> Result<Vec<u8>, FontAssetsError>,
) -> Result<&'static Configured, FontAssetsError> {
    let mut faces = Vec::with_capacity(TEXT_ROLES.len());
    for spec in &TEXT_ROLES {
        let name = file_for(roles, spec.role, spec.file)?;
        let bytes = read(spec.role, &name)?;
        faces.push(BodyFace::new(spec.family, spec.script, spec.style, bytes));
    }
    let mut latin_modern = None;
    let mut xits = None;
    for (role, family) in MATH_ROLES {
        let name = file_for(roles, role, expected_math_file(family))?;
        let bytes = read(role, &name)?;
        match family {
            MathFontFamily::LmMath => latin_modern = Some(bytes),
            _ => xits = Some(bytes),
        }
    }
    let (Some(latin_modern), Some(xits)) = (latin_modern, xits)
    else {
        return Err(FontAssetsError::ManifestInvalid("manifest is missing a math role".to_string()));
    };
    document_fonts::install_math_fonts(&MathFontPack::new(leak(latin_modern), leak(xits)));
    let set = Arc::new(BodyFaceSet::new(faces));
    let metrics = BodyMetrics::from_set(&set);
    Ok(Box::leak(Box::new(Configured { source, set, metrics })))
}

/// Production file name that carries one math family.
fn expected_math_file(family: MathFontFamily) -> &'static str {
    match family {
        MathFontFamily::XitsMath => "XITSMath-Regular.otf",
        _ => "LatinModernMath-Regular.otf",
    }
}

/// Leak one payload for the process-wide math registry.
fn leak(bytes: Vec<u8>) -> &'static [u8] {
    Box::leak(bytes.into_boxed_slice())
}

/// Lock the registry, leaving it untouched when a different source already won.
fn install(
    source: String,
    roles: &BTreeMap<String, String>,
    read: impl FnMut(&str, &str) -> Result<Vec<u8>, FontAssetsError>,
) -> Result<(), FontAssetsError> {
    let mut registry =
        CONFIGURED.lock().map_err(|_| FontAssetsError::ManifestInvalid("font asset registry is poisoned".to_string()))?;
    if let Some(existing) = *registry {
        if existing.source == source {
            return Ok(());
        }
        return Err(FontAssetsError::AlreadyConfigured(format!(
            "font assets were already configured from `{}`",
            existing.source
        )));
    }
    let configured = assemble(source, roles, read)?;
    *registry = Some(configured);
    Ok(())
}

/// Configure fonts from payloads the host read (native path P1).
///
/// `manifest_json` is the asset manifest, `font_buffers` maps each file name it
/// declares to the bytes the host loaded from `@vutex/vutex-fonts`. A process
/// accepts one source. Repeating the same manifest is a no-op, a different one
/// fails with `VUTEX_FONT_ASSETS_ALREADY_CONFIGURED`.
pub(crate) fn configure_from_bytes(manifest_json: &str, font_buffers: HashMap<String, Buffer>) -> Result<(), FontAssetsError> {
    let roles = parse_manifest(manifest_json)?;
    let source = format!("bytes:{manifest_json}");
    install(source, &roles, |role, name| {
        font_buffers
            .get(name)
            .map(|buffer| buffer.to_vec())
            .ok_or_else(|| FontAssetsError::FileMissing(format!("missing buffer `{name}` for role `{role}`")))
    })
}

/// Configure fonts from a directory the host names (native path P2).
///
/// Without `manifest_json` the directory must carry `fonts.json`. With one, it
/// is either a full manifest or a `role → file` mapping, which is how a system
/// font directory with different file names is consumed. Only the named files
/// are read, never a directory scan.
pub(crate) fn configure_from_dir(dir: &Path, manifest_json: Option<&str>) -> Result<(), FontAssetsError> {
    let manifest_raw = match manifest_json {
        Some(raw) => raw.to_string(),
        None => {
            let path = dir.join("fonts.json");
            std::fs::read_to_string(&path)
                .map_err(|error| FontAssetsError::ManifestInvalid(format!("cannot read `{}`: {error}", path.display())))?
        }
    };
    let roles = parse_manifest(&manifest_raw)?;
    let canonical: PathBuf = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    let source = format!("dir:{}\n{manifest_raw}", canonical.display());
    install(source, &roles, |role, name| {
        let path = dir.join(name);
        std::fs::read(&path)
            .map_err(|_| FontAssetsError::FileMissing(format!("missing file `{}` for role `{role}`", path.display())))
    })
}

/// Registry entry when a host configured one.
fn configured() -> Option<&'static Configured> {
    *CONFIGURED.lock().expect("font asset registry")
}

/// Embedded fallback that predates host configuration.
fn embedded_set() -> &'static Arc<BodyFaceSet> {
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

/// Embedded body metrics, parsed once.
fn embedded_metrics() -> &'static BodyMetrics {
    static METRICS: OnceLock<BodyMetrics> = OnceLock::new();
    METRICS.get_or_init(|| BodyMetrics::from_set(embedded_set()))
}

/// Shared handle for [`PdfFontPolicy::Provided`](vutex_render_pdf::PdfFontPolicy).
pub(crate) fn body_set() -> Arc<BodyFaceSet> {
    match configured() {
        Some(configured) => configured.set.clone(),
        None => embedded_set().clone(),
    }
}

/// Faces in the pack's declaration order.
pub(crate) fn body_faces() -> &'static [BodyFace] {
    match configured() {
        Some(configured) => configured.set.faces(),
        None => embedded_set().faces(),
    }
}

/// Metrics of the pack, parsed once.
pub(crate) fn body_metrics() -> &'static BodyMetrics {
    match configured() {
        Some(configured) => &configured.metrics,
        None => embedded_metrics(),
    }
}

/// Resolve a bundled math package id to its family.
pub(crate) fn math_family(package: &str) -> Option<MathFontFamily> {
    MATH_ROLES.iter().find(|(id, _)| *id == package).map(|(_, family)| *family)
}

// Embedded payloads kept until the explicit configuration cut-over lands.
const TINOS_REGULAR: &[u8] = include_bytes!("../../../fonts/Tinos-Regular.ttf");
const TINOS_BOLD: &[u8] = include_bytes!("../../../fonts/Tinos-Bold.ttf");
const TINOS_ITALIC: &[u8] = include_bytes!("../../../fonts/Tinos-Italic.ttf");
const NOTO_SERIF_SC_REGULAR: &[u8] = include_bytes!("../../../fonts/NotoSerifSC-Regular.ttf");
const NOTO_SERIF_SC_BOLD: &[u8] = include_bytes!("../../../fonts/NotoSerifSC-Bold.ttf");
const NOTO_SANS_MONO_REGULAR: &[u8] = include_bytes!("../../../fonts/NotoSansMono-Regular.ttf");
const LATIN_MODERN_MATH: &[u8] = include_bytes!("../../../fonts/LatinModernMath-Regular.otf");
const XITS_MATH: &[u8] = include_bytes!("../../../fonts/XITSMath-Regular.otf");

/// Math faces installed into `document_text` at startup.
pub(crate) fn math_pack() -> MathFontPack {
    MathFontPack::new(LATIN_MODERN_MATH, XITS_MATH)
}
