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
//! font binaries. Latin documents do not carry the CJK faces, which run to
//! megabytes, so the payload only pays for the scripts it actually paints.

use document_fonts::{BodyFace, BodyFaceStyle, BodyFamily, BodyScript, BODY_FACES};

/// CSS family name the Latin reading faces are published under.
pub const SERIF_FAMILY: &str = "VuTeX Serif";
/// CSS family name the fixed-advance face is published under.
pub const MONO_FAMILY: &str = "VuTeX Mono";
/// CSS family name the Han faces are published under.
pub const CJK_FAMILY: &str = "VuTeX Serif CJK";

/// Embed the bundled body faces into an SVG payload and name them on every run.
///
/// The renderer tags prose runs `font-family="serif"` and code runs
/// `font-family="monospace"`. Both tags are rewritten to the bundled family
/// names, and a `<style>` block carrying the face bytes is inserted after the
/// document title. CJK faces are only embedded when the payload paints a Han
/// character, so a Latin-only document does not grow by megabytes of outlines.
pub fn embed_body_fonts(svg: &str) -> String {
    let has_cjk = svg.chars().any(is_cjk);
    let mut families = String::new();
    for face in BODY_FACES {
        if face.script == BodyScript::Cjk && !has_cjk {
            continue;
        }
        families.push_str(&face_rule(face));
    }

    let serif_chain = if has_cjk { format!("{SERIF_FAMILY}, {CJK_FAMILY}") } else { SERIF_FAMILY.to_string() };
    let mono_chain = if has_cjk { format!("{MONO_FAMILY}, {CJK_FAMILY}") } else { MONO_FAMILY.to_string() };

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

    with_style
        .replace("font-family=\"serif\"", &format!("font-family=\"{serif_chain}\""))
        .replace("font-family=\"monospace\"", &format!("font-family=\"{mono_chain}\""))
}

/// One `@font-face` rule for a bundled face, carrying the file as a data URI.
fn face_rule(face: &BodyFace) -> String {
    let family = match (face.family, face.script) {
        (BodyFamily::Serif, BodyScript::Latin) => SERIF_FAMILY,
        (BodyFamily::Serif, BodyScript::Cjk) => CJK_FAMILY,
        (BodyFamily::Mono, _) => MONO_FAMILY,
    };
    let style = if face.style == BodyFaceStyle::Italic { "italic" } else { "normal" };
    let weight = if face.style == BodyFaceStyle::Bold { 700 } else { 400 };
    let data = encode_base64(face.bytes);
    format!(
        "@font-face{{font-family:'{family}';font-style:{style};font-weight:{weight};\
         src:url(data:font/ttf;base64,{data}) format('truetype');}}"
    )
}

/// Whether a character belongs to the Han/CJK ranges the bundled CJK faces cover.
fn is_cjk(ch: char) -> bool {
    matches!(ch as u32,
        0x2E80..=0x2FFF
        | 0x3000..=0x303F
        | 0x3040..=0x9FFF
        | 0x3400..=0x4DBF
        | 0xF900..=0xFAFF
        | 0xFE30..=0xFE4F
        | 0xFF00..=0xFFEF)
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
        assert!(!out.contains("NotoSerifSC"), "{out}");
        assert_eq!(out.matches("@font-face").count(), 4, "Latin serif 3 faces plus mono 1 face");
    }

    #[test]
    fn han_payload_embeds_the_cjk_chain() {
        let svg =
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><title>VuTeX document</title><text font-family=\"serif\">中文正文</text></svg>";
        let out = embed_body_fonts(svg);
        assert!(out.contains(CJK_FAMILY), "{out}");
        assert!(out.contains(&format!("font-family=\"{SERIF_FAMILY}, {CJK_FAMILY}\"")), "{out}");
        assert_eq!(out.matches("@font-face").count(), 6, "three Latin plus two CJK plus mono");
    }

    #[test]
    fn style_goes_after_the_title_so_it_precedes_the_first_run() {
        let svg = "<svg xmlns=\"http://www.w3.org/2000/svg\"><title>VuTeX document</title><text>Body</text></svg>";
        let out = embed_body_fonts(svg);
        let style = out.find("<style").expect("style block");
        let first_run = out.find("<text").expect("text run");
        assert!(style < first_run, "{out}");
    }
}
