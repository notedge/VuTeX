//! Font embedding for the SVG document backend.
//!
//! `document-layout` breaks lines against [`document_fonts::BODY_FACES`], but
//! `document-renderer-svg` names only the generic CSS families `serif` and
//! `monospace` on each `<text>`. A host resolves those generics to whatever
//! face its locale prefers, so the painted advance can differ from the one the
//! line breaker reserved: on a Simplified Chinese host both generics resolve to
//! SimSun/NSimSun, whose Latin advances are half an em, and the wider paint
//! overlaps the following run and overflows the right margin.
//!
//! This module closes the gap inside the payload: it names the bundled faces
//! explicitly and carries their bytes as `@font-face` data URIs, so the SVG is
//! self-contained and paints with the same faces the layout measured. The faces
//! come from `document_fonts::BODY_FACES`, the single place this family ships
//! font binaries. Only the characters a document paints are kept, through
//! [`crate::font_subset`], so a Latin page does not carry the CJK faces and a
//! page pays for the glyphs it uses rather than for the whole face.

use std::collections::BTreeSet;

use document_fonts::{BodyFace, BodyFaceStyle, BodyFamily, BodyScript, BODY_FACES};

use crate::font_subset::{mapped, subset_font, FacePayload};

/// CSS family name the Latin reading faces are published under.
pub const SERIF_FAMILY: &str = "VuTeX Serif";
/// CSS family name the fixed-advance face is published under.
pub const MONO_FAMILY: &str = "VuTeX Mono";
/// CSS family name the Han faces are published under.
pub const CJK_FAMILY: &str = "VuTeX Serif CJK";

/// A `<text>` run as the SVG backend emitted it.
struct TextRun {
    family: String,
    weight: u32,
    italic: bool,
    chars: BTreeSet<char>,
}

/// Embed the bundled body faces into an SVG payload and name them on every run.
///
/// The renderer tags prose runs `font-family="serif"` and code runs
/// `font-family="monospace"`. Both tags are rewritten to the bundled family
/// names, and a `<style>` block carrying a per-document subset of each face is
/// inserted after the document title. A face whose character set is empty is
/// left out, so a document without code does not carry the mono face.
pub fn embed_body_fonts(svg: &str) -> String {
    let mut sets: Vec<BTreeSet<char>> = vec![BTreeSet::new(); BODY_FACES.len()];
    for run in text_runs(svg) {
        let bold = run.weight >= 600;
        let mut remaining = run.chars;
        for index in candidates(&run.family, bold, run.italic) {
            if remaining.is_empty() {
                break;
            }
            let covered = mapped(BODY_FACES[index].bytes, &remaining);
            remaining.retain(|ch| !covered.contains(ch));
            sets[index].extend(covered);
        }
    }

    let mut families = String::new();
    let mut latin_serif = false;
    let mut mono = false;
    let mut cjk = false;
    for (face, chars) in BODY_FACES.iter().zip(sets.iter()) {
        if chars.is_empty() {
            continue;
        }
        let bytes = match subset_font(face.bytes, chars) {
            Some(FacePayload::Subset(subset)) => subset.bytes,
            Some(FacePayload::Full) => face.bytes.to_vec(),
            None => continue,
        };
        families.push_str(&face_rule(face, &bytes));
        match (face.family, face.script) {
            (BodyFamily::Serif, BodyScript::Latin) => latin_serif = true,
            (BodyFamily::Serif, BodyScript::Cjk) => cjk = true,
            (BodyFamily::Mono, _) => mono = true,
        }
    }

    let mut chain = Vec::new();
    if latin_serif {
        chain.push(SERIF_FAMILY);
    }
    if cjk {
        chain.push(CJK_FAMILY);
    }
    let serif_chain = chain.join(", ");
    let mut mono_chain = Vec::new();
    if mono {
        mono_chain.push(MONO_FAMILY);
    }
    if cjk {
        mono_chain.push(CJK_FAMILY);
    }
    let mono_chain = mono_chain.join(", ");

    let style = format!("<style type=\"text/css\">{families}</style>");
    let with_style = match svg.find("</title>") {
        Some(index) => {
            let split = index + "</title>".len();
            format!("{}{}{}", &svg[..split], style, &svg[split..])
        }
        None => match svg.find('>') {
            Some(index) => format!("{}{}{}", &svg[..=index], style, &svg[index + 1..]),
            None => return svg.to_string(),
        },
    };

    let mut out = with_style;
    if !serif_chain.is_empty() {
        out = out.replace("font-family=\"serif\"", &format!("font-family=\"{serif_chain}\""));
    }
    if !mono_chain.is_empty() {
        out = out.replace("font-family=\"monospace\"", &format!("font-family=\"{mono_chain}\""));
    }
    out
}

/// Read every `<text>` run with its style and its decoded characters.
fn text_runs(svg: &str) -> Vec<TextRun> {
    let mut runs = Vec::new();
    let mut cursor = 0usize;
    while let Some(found) = svg[cursor..].find("<text") {
        let start = cursor + found;
        let after = svg[start + 5..].chars().next();
        if !after.map(|ch| ch.is_whitespace() || ch == '>').unwrap_or(false) {
            cursor = start + 5;
            continue;
        }
        let Some(tag_end) = svg[start..].find('>').map(|index| start + index) else {
            break;
        };
        let Some(close) = svg[tag_end..].find("</text>").map(|index| tag_end + index) else {
            break;
        };
        let tag = &svg[start..=tag_end];
        let family = attribute(tag, "font-family").unwrap_or("serif").to_string();
        let weight = attribute(tag, "font-weight").and_then(|value| value.parse::<u32>().ok()).unwrap_or(400);
        let italic = attribute(tag, "font-style") == Some("italic");
        let chars = decode_entities(&svg[tag_end + 1..close]).chars().collect();
        runs.push(TextRun { family, weight, italic, chars });
        cursor = close + "</text>".len();
    }
    runs
}

fn attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let needle = format!("{name}=\"");
    let start = tag.find(&needle)? + needle.len();
    let rest = &tag[start..];
    let end = rest.find('"')?;
    Some(&rest[..end])
}

fn decode_entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

fn face_index(family: BodyFamily, script: BodyScript, style: BodyFaceStyle) -> Option<usize> {
    BODY_FACES
        .iter()
        .position(|face| face.family == family && face.script == script && face.style == style)
}

/// Faces a run consults, in the order CSS font matching picks them.
///
/// The style face comes first, then its family's regular face (the weight
/// fallback a browser takes when a bold or italic face misses a glyph), then
/// the Han faces in the fallback chain the rewritten family list declares.
fn candidates(family: &str, bold: bool, italic: bool) -> Vec<usize> {
    let mut order = Vec::new();
    let mut primary = Vec::new();
    if family == "monospace" {
        primary.push((BodyFamily::Mono, BodyScript::Latin, BodyFaceStyle::Regular));
    } else {
        if bold {
            primary.push((BodyFamily::Serif, BodyScript::Latin, BodyFaceStyle::Bold));
        } else if italic {
            primary.push((BodyFamily::Serif, BodyScript::Latin, BodyFaceStyle::Italic));
        }
        primary.push((BodyFamily::Serif, BodyScript::Latin, BodyFaceStyle::Regular));
    }
    for (face_family, script, style) in primary {
        if let Some(index) = face_index(face_family, script, style) {
            order.push(index);
        }
    }
    if bold {
        if let Some(index) = face_index(BodyFamily::Serif, BodyScript::Cjk, BodyFaceStyle::Bold) {
            order.push(index);
        }
    }
    if let Some(index) = face_index(BodyFamily::Serif, BodyScript::Cjk, BodyFaceStyle::Regular) {
        order.push(index);
    }
    order
}

/// One `@font-face` rule for a bundled face, carrying the payload as a data URI.
fn face_rule(face: &BodyFace, bytes: &[u8]) -> String {
    let family = match (face.family, face.script) {
        (BodyFamily::Serif, BodyScript::Latin) => SERIF_FAMILY,
        (BodyFamily::Serif, BodyScript::Cjk) => CJK_FAMILY,
        (BodyFamily::Mono, _) => MONO_FAMILY,
    };
    let style = if face.style == BodyFaceStyle::Italic { "italic" } else { "normal" };
    let weight = if face.style == BodyFaceStyle::Bold { 700 } else { 400 };
    let data = encode_base64(bytes);
    format!(
        "@font-face{{font-family:'{family}';font-style:{style};font-weight:{weight};\
         src:url(data:font/ttf;base64,{data}) format('truetype');}}"
    )
}

/// Encode bytes as standard base64, padding with `=`.
fn encode_base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let (b0, b1, b2) = (chunk[0], chunk.get(1).copied(), chunk.get(2).copied());
        let triple = (u32::from(b0) << 16) | (u32::from(b1.unwrap_or(0)) << 8) | u32::from(b2.unwrap_or(0));
        out.push(ALPHABET[(triple >> 18) as usize & 0x3F] as char);
        out.push(ALPHABET[(triple >> 12) as usize & 0x3F] as char);
        out.push(if b1.is_some() { ALPHABET[(triple >> 6) as usize & 0x3F] as char } else { '=' });
        out.push(if b2.is_some() { ALPHABET[triple as usize & 0x3F] as char } else { '=' });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_known_vectors() {
        assert_eq!(encode_base64(b""), "");
        assert_eq!(encode_base64(b"f"), "Zg==");
        assert_eq!(encode_base64(b"fo"), "Zm8=");
        assert_eq!(encode_base64(b"foo"), "Zm9v");
        assert_eq!(encode_base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn latin_only_payload_rewrites_generics_without_cjk_faces() {
        let svg =
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><title>VuTeX document</title><text font-family=\"serif\">Body</text><text font-family=\"monospace\">code</text></svg>";
        let out = embed_body_fonts(svg);
        assert!(out.contains("<style type=\"text/css\">"), "{out}");
        assert!(out.contains(&format!("font-family=\"{SERIF_FAMILY}\"")), "{out}");
        assert!(out.contains(&format!("font-family=\"{MONO_FAMILY}\"")), "{out}");
        assert!(!out.contains("font-family=\"serif\""), "{out}");
        assert!(!out.contains("font-family=\"monospace\""), "{out}");
        assert!(!out.contains(CJK_FAMILY), "{out}");
        assert_eq!(out.matches("@font-face").count(), 2, "Tinos regular plus Noto Sans Mono");
    }

    #[test]
    fn han_payload_embeds_the_cjk_chain() {
        let svg =
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><title>VuTeX document</title><text font-family=\"serif\">中文正文</text></svg>";
        let out = embed_body_fonts(svg);
        assert!(out.contains(CJK_FAMILY), "{out}");
        assert!(out.contains(&format!("font-family=\"{CJK_FAMILY}\"")), "{out}");
        assert!(!out.contains("font-family:'VuTeX Serif';"), "Tinos does not map Han so it must not be embedded: {out}");
        assert_eq!(out.matches("@font-face").count(), 1, "{out}");
    }

    #[test]
    fn a_mixed_run_splits_characters_across_the_chain() {
        let svg = "<svg xmlns=\"http://www.w3.org/2000/svg\"><title>VuTeX document</title><text font-family=\"serif\">Body 中文</text></svg>";
        let out = embed_body_fonts(svg);
        assert!(out.contains(SERIF_FAMILY), "{out}");
        assert!(out.contains(CJK_FAMILY), "{out}");
        assert!(out.contains(&format!("font-family=\"{SERIF_FAMILY}, {CJK_FAMILY}\"")), "{out}");
        assert_eq!(out.matches("@font-face").count(), 2, "{out}");
    }

    #[test]
    fn style_goes_after_the_title_so_it_precedes_the_first_run() {
        let svg = "<svg xmlns=\"http://www.w3.org/2000/svg\"><title>VuTeX document</title><text>Body</text></svg>";
        let out = embed_body_fonts(svg);
        let style = out.find("<style").expect("style block");
        let first_run = out.find("<text").expect("text run");
        assert!(style < first_run, "{out}");
    }

    #[test]
    fn unused_faces_are_not_embedded() {
        let svg =
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><title>VuTeX document</title><text font-family=\"serif\">Body</text></svg>";
        let out = embed_body_fonts(svg);
        assert_eq!(out.matches("@font-face").count(), 1, "only the regular serif face is used");
        assert!(!out.contains("font-weight:700"), "{out}");
        assert!(!out.contains("font-style:italic"), "{out}");
        assert!(!out.contains(MONO_FAMILY), "{out}");
    }
}
