//! OpenType MATH oracle: fontTools versus VuTeX's own font reader.
//!
//! `tests/fixtures/oracle/latinmodern-math-facts.json` is produced by
//! `scripts/oracle/export-math-facts.py` — a Python/fontTools program that has
//! no code in common with VuTeX — and this test checks that the values
//! `vutex-text` derives from the same font file agree with it.
//!
//! That is what makes the comparison an oracle instead of self-verification:
//! an incorrect MATH lookup in Rust fails here even though every VuTeX-internal
//! test would still pass.
//!
//! Sources: font units stay raw in the fixture (fontTools reports them verbatim)
//! and are converted to em here, so the fixture never encodes a normalization
//! rule that could drift from the engine under test.

use std::{collections::BTreeMap, fs, path::Path};

use serde::Deserialize;
use vutex_text::math::opentype_font_metrics;
use vutex_types::math::{FontStyle, GlyphKey, MathConstant};

/// Font family label `vutex-text` attaches to every glyph it extracts.
const FAMILY: &str = "latin-modern-math";

/// Absolute tolerance in em for every compared value.
///
/// `Fixed` is 16.16, so its quantum is 1/65536 ≈ 1.526e-5 em and a single
/// f64 → `Fixed` conversion error stays below that. Latin Modern Math is
/// 1000 units/em, so one design unit is 1e-3 em: a tolerance of 1e-4 em is
/// roughly six quanta and still strictly smaller than one design unit. Any
/// off-by-one-unit mistake in either reader therefore fails the comparison,
/// while pure representation rounding does not.
const TOLERANCE: f64 = 1e-4;

/// Font asset the `vutex-text` crate embeds.
fn font_bytes() -> Vec<u8> {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("vutex-text").join("assets").join("LatinModernMath-Regular.otf");
    fs::read(&path).unwrap_or_else(|e| panic!("failed to read font {}: {e}", path.display()))
}

/// Path of the fontTools snapshot.
fn fixture_path() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join("oracle").join("latinmodern-math-facts.json")
}

#[derive(Debug, Deserialize)]
struct Facts {
    schema_version: String,
    source: Source,
    font: FontFact,
    math: MathFacts,
}

#[derive(Debug, Deserialize)]
struct Source {
    engine: String,
    version: Option<String>,
    status: String,
    status_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FontFact {
    path: String,
    sha256: String,
    size_bytes: u64,
    family: String,
    units_per_em: u16,
}

#[derive(Debug, Deserialize)]
struct MathFacts {
    constants: BTreeMap<String, ConstantFact>,
    glyphs: Vec<GlyphFact>,
    variants: Vec<VariantFact>,
    math_kern_info: KernInfoFact,
}

#[derive(Debug, Deserialize)]
struct ConstantFact {
    value: i64,
    unit: String,
}

#[derive(Debug, Deserialize)]
struct GlyphFact {
    #[serde(rename = "char")]
    ch: String,
    glyph_name: String,
    advance: i64,
    bounds: Option<BoundsFact>,
    italic_correction: Option<i64>,
    top_accent_attachment: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct BoundsFact {
    y_min: i64,
    y_max: i64,
}

#[derive(Debug, Deserialize)]
struct VariantFact {
    #[serde(rename = "char")]
    ch: String,
    glyph_name: String,
    vertical: Option<ConstructionFact>,
}

#[derive(Debug, Deserialize)]
struct ConstructionFact {
    variants: Vec<VariantGlyphFact>,
    assembly: Option<AssemblyFact>,
}

#[derive(Debug, Deserialize)]
struct VariantGlyphFact {
    glyph_name: String,
    glyph_id: u16,
    advance_measurement: i64,
    hmtx_advance: i64,
}

#[derive(Debug, Deserialize)]
struct AssemblyFact {
    parts: Vec<PartFact>,
}

#[derive(Debug, Deserialize)]
struct PartFact {
    glyph_name: String,
    glyph_id: u16,
    start_connector_length: i64,
    end_connector_length: i64,
    full_advance: i64,
    is_extender: bool,
}

#[derive(Debug, Deserialize)]
struct KernInfoFact {
    present: bool,
    reason: Option<String>,
}

/// Load the fontTools snapshot.
fn load_facts() -> Facts {
    let path = fixture_path();
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| {
        panic!("{} must be a fontTools snapshot; regenerate it with scripts/oracle/export-math-facts.py: {e}", path.display())
    })
}

/// Assert that two em-normalized values agree within the documented tolerance.
fn assert_close(label: &str, actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= TOLERANCE,
        "{label}: VuTeX reports {actual}, the external snapshot says {expected} (difference {})",
        actual - expected
    );
}

/// The snapshot must be a successful fontTools run against the shipped font.
///
/// If the exporter ever wrote `blocked`, or measured a different font file, the
/// numeric comparisons below would be comparing against nothing.
#[test]
fn snapshot_comes_from_fonttools_and_covers_the_shipped_font() {
    let facts = load_facts();
    assert_eq!(facts.schema_version, "v0");
    assert_eq!(facts.source.engine, "fonttools", "the MATH snapshot must come from fontTools");
    assert_eq!(facts.source.status, "available", "a blocked snapshot carries no MATH evidence");
    assert!(facts.source.version.is_some(), "fontTools reports its own version, so the snapshot must record it");
    assert!(facts.source.status_reason.is_none(), "an available snapshot has no blocker to explain");

    assert_eq!(
        facts.font.path, "projects/crates/vutex-text/assets/LatinModernMath-Regular.otf",
        "the snapshot must measure exactly the font the crate embeds"
    );
    assert!(!facts.math.constants.is_empty(), "the snapshot must contain MATH constants");
    assert!(!facts.math.glyphs.is_empty(), "the snapshot must cover glyphs");
    assert_eq!(facts.font.units_per_em, 1000, "Latin Modern Math is a 1000 unit/em font");
    assert_eq!(facts.font.family, "Latin Modern Math", "the snapshot must name the font it measured");

    let bytes = font_bytes();
    assert_eq!(facts.font.size_bytes as usize, bytes.len(), "font size changed since the snapshot was generated");
    assert_eq!(
        facts.font.sha256,
        sha256_hex(&bytes),
        "the snapshot was generated against a different font binary; regenerate it with scripts/oracle/export-math-facts.py"
    );
}

/// Every MATH constant fontTools reads must land in `vutex-text` unchanged.
#[test]
fn math_constants_match_the_fonttools_snapshot() {
    let facts = load_facts();
    let data = opentype_font_metrics().expect("the embedded font must parse and expose its MATH table");

    assert_eq!(data.units_per_em, facts.font.units_per_em, "design units per em must come from the font");
    assert_eq!(data.family, FAMILY);
    assert!(!facts.math.constants.is_empty(), "the snapshot must contain MATH constants");

    let one_em = f64::from(facts.font.units_per_em);
    for (name, fact) in &facts.math.constants {
        let constant = constant_by_name(name);
        let expected = if fact.unit == "percent" {
            // The two script-scale and one degree-raise constants are the only
            // spec percentages; VuTeX stores them as a 0–1 fraction.
            fact.value as f64 / 100.0
        }
        else {
            fact.value as f64 / one_em
        };
        let actual = data
            .constants
            .get(&constant)
            .unwrap_or_else(|| panic!("`vutex-text` did not extract the MATH constant `{name}`"))
            .to_f64();
        assert_close(&format!("MATH constant `{name}` ({})", fact.unit), actual, expected);
    }
}

/// Map a fixture constant name onto the crate's [`MathConstant`].
///
/// The enum derives `Deserialize`, so this both resolves the constant and
/// proves the fixture does not name a constant the crate has never heard of.
fn constant_by_name(name: &str) -> MathConstant {
    serde_json::from_value(serde_json::Value::String(name.to_string()))
        .unwrap_or_else(|e| panic!("fixture constant `{name}` is not a `MathConstant` in this crate: {e}"))
}

/// Glyph advances, ink extents, italic corrections and accent attachments must
/// match fontTools.
#[test]
fn glyph_metrics_match_the_fonttools_snapshot() {
    let facts = load_facts();
    let data = opentype_font_metrics().expect("the embedded font must parse");
    let one_em = f64::from(facts.font.units_per_em);

    assert!(!facts.math.glyphs.is_empty(), "the snapshot must cover glyphs");
    for fact in &facts.math.glyphs {
        let ch = single_char(&fact.ch, &fact.glyph_name);
        let bounds = fact.bounds.as_ref().unwrap_or_else(|| {
            panic!(
                "`{}` ({}) has no outline bounds in the snapshot, so the oracle cannot check height and depth",
                fact.ch, fact.glyph_name
            )
        });
        let metrics = math_metrics(&data, ch);

        assert_close(&format!("`{}` advance", fact.ch), metrics.width.to_f64(), fact.advance as f64 / one_em);
        assert_close(&format!("`{}` height", fact.ch), metrics.height.to_f64(), (bounds.y_max.max(0) as f64) / one_em);
        assert_close(&format!("`{}` depth", fact.ch), metrics.depth.to_f64(), ((-bounds.y_min).max(0) as f64) / one_em);

        // Absent MathGlyphInfo coverage is a missing kern, not a zero kern, so
        // the extractor must fall back to zero only because nothing was found.
        let expected_italic = fact.italic_correction.unwrap_or(0) as f64 / one_em;
        let expected_accent = fact.top_accent_attachment.unwrap_or(0) as f64 / one_em;
        assert_close(&format!("`{}` italic correction", fact.ch), metrics.italic_correction.to_f64(), expected_italic);
        assert_close(&format!("`{}` top accent attachment", fact.ch), metrics.top_accent_attachment.to_f64(), expected_accent);

        // `vutex-text` registers the same metrics under the normal face so
        // accent marks resolve without a second lookup. Pin that here so the
        // two keys cannot silently diverge.
        let normal = data
            .glyphs
            .get(&GlyphKey::from_char(ch, Some(FAMILY.to_string()), FontStyle::Normal))
            .unwrap_or_else(|| panic!("`{}` has no normal-face metrics", fact.ch));
        assert_eq!(*normal, *metrics, "`{}` must carry identical metrics under both faces", fact.ch);
    }
}

/// Vertical MATH Variants and assemblies must match fontTools glyph for glyph.
#[test]
fn delimiter_constructions_match_the_fonttools_snapshot() {
    let facts = load_facts();
    let data = opentype_font_metrics().expect("the embedded font must parse");
    let one_em = f64::from(facts.font.units_per_em);
    let family = Some(FAMILY.to_string());

    assert!(!facts.math.variants.is_empty(), "the snapshot must cover delimiter constructions");
    let mut widest_stretch_gap = 0.0f64;

    for fact in &facts.math.variants {
        let ch = single_char(&fact.ch, &fact.glyph_name);
        let construction = fact.vertical.as_ref().unwrap_or_else(|| {
            panic!("`{}` has no vertical construction in the snapshot, so there is nothing to compare", fact.ch)
        });

        let extracted = data
            .delimiters
            .get(&GlyphKey::from_char(ch, family.clone(), FontStyle::Math))
            .unwrap_or_else(|| panic!("`vutex-text` extracted no construction for `{}`", fact.ch));

        assert_eq!(
            extracted.variants.len(),
            construction.variants.len(),
            "`{}` ({}) must expose every prepared variant, the snapshot lists {}",
            fact.ch,
            fact.glyph_name,
            construction.variants.len()
        );

        for (source, extracted) in construction.variants.iter().zip(&extracted.variants) {
            assert_eq!(
                extracted.glyph.glyph_id,
                Some(source.glyph_id),
                "`{}` variant glyph ids must follow the font order",
                fact.ch
            );
            assert_eq!(extracted.glyph.char, Some(ch), "`{}` variants keep their owning character", fact.ch);
            assert_eq!(extracted.glyph.style, FontStyle::Math);

            // A vertical variant advances horizontally by its `hmtx` width,
            // never by the stacked `advance_measurement`.
            let metrics = data
                .glyphs
                .get(&extracted.glyph)
                .unwrap_or_else(|| panic!("`{}` variant {} has no metrics", fact.ch, source.glyph_name));
            assert_close(
                &format!("`{}` variant {} advance", fact.ch, source.glyph_name),
                metrics.width.to_f64(),
                source.hmtx_advance as f64 / one_em,
            );
            widest_stretch_gap =
                widest_stretch_gap.max(source.advance_measurement as f64 / one_em - source.hmtx_advance as f64 / one_em);
        }

        match &construction.assembly {
            Some(_) => assert!(
                !extracted.components.is_empty(),
                "`{}` has a piecewise assembly in the font but `vutex-text` extracted no parts",
                fact.ch
            ),
            None => assert!(
                extracted.components.is_empty(),
                "`{}` has no assembly in the font but `vutex-text` extracted {} parts",
                fact.ch,
                extracted.components.len()
            ),
        }
        let Some(assembly) = construction.assembly.as_ref()
        else {
            continue;
        };
        assert_eq!(
            extracted.components.len(),
            assembly.parts.len(),
            "`{}` ({}) must keep every assembly part",
            fact.ch,
            fact.glyph_name
        );
        for (source, extracted) in assembly.parts.iter().zip(&extracted.components) {
            assert_eq!(extracted.glyph.glyph_id, Some(source.glyph_id), "`{}` part glyph ids must follow the font", fact.ch);
            assert_eq!(
                extracted.is_extender, source.is_extender,
                "`{}` part {}: extender flag comes from the font part flags",
                fact.ch, source.glyph_name
            );
            assert_close(
                &format!("`{}` part {} start connector", fact.ch, source.glyph_name),
                extracted.start_connector.to_f64(),
                source.start_connector_length as f64 / one_em,
            );
            assert_close(
                &format!("`{}` part {} end connector", fact.ch, source.glyph_name),
                extracted.end_connector.to_f64(),
                source.end_connector_length as f64 / one_em,
            );
            assert_close(
                &format!("`{}` part {} full advance", fact.ch, source.glyph_name),
                extracted.full_advance.to_f64(),
                source.full_advance as f64 / one_em,
            );
        }
    }

    assert!(
        widest_stretch_gap > 1.0,
        "no variant's stacked advance exceeds its horizontal advance by an em, so the constructions in this snapshot \
         cannot tell a stacking measurement from an advance"
    );
}

/// Latin Modern Math publishes no `MathKernInfo`, so VuTeX must extract no kern.
///
/// A non-empty kern map here would mean a fabricated kern: the external tool
/// proves the font has no corner kern data at all.
#[test]
fn math_kern_data_follows_the_fonttools_snapshot() {
    let facts = load_facts();
    let data = opentype_font_metrics().expect("the embedded font must parse");

    assert!(
        !facts.math.math_kern_info.present,
        "the snapshot claims this font has kern data, so the emptiness assertion below no longer describes it"
    );
    assert_eq!(
        facts.math.math_kern_info.reason.as_deref().map(str::trim).filter(|r| !r.is_empty()),
        Some("the font publishes no MathKernInfo subtable, so no corner kern exists at all"),
        "a missing kern table must be explained, not left blank"
    );
    assert!(
        data.kern_map.is_empty(),
        "`vutex-text` invented {} corner kernels for a font without MathKernInfo",
        data.kern_map.len()
    );
}

/// The single character a fixture glyph or variant entry describes.
fn single_char(ch: &str, glyph_name: &str) -> char {
    let mut chars = ch.chars();
    let ch = chars.next().unwrap_or_else(|| panic!("glyph `{glyph_name}` has no character"));
    assert!(chars.next().is_none(), "glyph `{glyph_name}` must describe exactly one character");
    ch
}

/// Metrics `vutex-text` reports for one character in the math face.
fn math_metrics<'a>(data: &'a vutex_types::math::FontMetricsData, ch: char) -> &'a vutex_types::math::GlyphMetrics {
    data.glyphs
        .get(&GlyphKey::from_char(ch, Some(FAMILY.to_string()), FontStyle::Math))
        .unwrap_or_else(|| panic!("`{ch}` has no math-face metrics"))
}

/// SHA-256 of a byte slice, lowercase hex.
///
/// The test crate has no hash dependency and the fixture has to be bound to the
/// font binary it was generated from, so FIPS 180-4 lives here in full.
fn sha256_hex(bytes: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01,
        0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc,
        0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147,
        0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08,
        0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
        0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];

    let mut state: [u32; 8] = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];

    let mut padded = bytes.to_vec();
    let bit_len = (bytes.len() as u64).wrapping_mul(8);
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in padded.chunks_exact(64) {
        let mut schedule = [0u32; 64];
        for (index, word) in chunk.chunks_exact(4).enumerate() {
            schedule[index] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for index in 16..64 {
            let s0 = schedule[index - 15].rotate_right(7) ^ schedule[index - 15].rotate_right(18) ^ (schedule[index - 15] >> 3);
            let s1 = schedule[index - 2].rotate_right(17) ^ schedule[index - 2].rotate_right(19) ^ (schedule[index - 2] >> 10);
            schedule[index] = schedule[index - 16].wrapping_add(s0).wrapping_add(schedule[index - 7]).wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = state;
        for index in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[index]).wrapping_add(schedule[index]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        for (slot, value) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *slot = slot.wrapping_add(value);
        }
    }

    let mut hex = String::with_capacity(64);
    for word in state {
        hex.push_str(&format!("{word:08x}"));
    }
    hex
}
