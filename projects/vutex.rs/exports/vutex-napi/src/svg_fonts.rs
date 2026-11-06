//! Font embedding shared by every SVG and HTML emission path.
//!
//! `document-layout` breaks lines against a measured face set, but the
//! `document-renderer-svg` and `document-renderer-html` backends name only the
//! generic CSS families `serif` and `monospace`. A host resolves those generics
//! to whatever face its locale prefers, so the painted advance can differ from
//! the one the line breaker reserved: on a Simplified Chinese host both
//! generics resolve to SimSun/NSimSun, whose Latin advances are half an em, and
//! the wider paint overlaps the following run and overflows the right margin.
//!
//! This module closes the gap inside the payload: it names the bundled faces
//! explicitly and carries their bytes as `@font-face` data URIs, so the payload
//! is self-contained and paints with the same faces the layout measured. The
//! faces come from the crate's own [`crate::assets`] pack, the single place this
//! repository ships font binaries. Only the characters a document paints are kept, through
//! [`crate::font_subset`], so a Latin page does not carry the CJK faces and a
//! page pays for the glyphs it uses rather than for the whole face.
//!
//! Every emission entry calls [`embed_body_fonts`] or [`embed_html_fonts`].
//! No caller assembles a font list of its own, which is what keeps a rebuilt
//! binding and the TypeScript CLI from drifting apart.

use std::collections::BTreeSet;

use document_fonts::{BodyFace, BodyFaceStyle, BodyFamily, BodyScript};

use crate::{
    assets::body_faces,
    font_subset::{FacePayload, mapped, subset_font},
    theme::Theme,
};

/// CSS family name the reading faces are published under.
pub const SERIF_FAMILY: &str = "VuTeX Serif";
/// CSS family name the fixed-advance face is published under.
pub const MONO_FAMILY: &str = "VuTeX Mono";
/// CSS family name the Han faces are published under in the traditional theme.
pub const CJK_FAMILY: &str = "VuTeX Serif CJK";
/// CSS family name that serves a glyph the requested style face lacks.
pub const FALLBACK_FAMILY: &str = "VuTeX Fallback";
/// CSS family name that serves a Han glyph the Latin fallback face lacks.
pub const FALLBACK_CJK_FAMILY: &str = "VuTeX Fallback CJK";

/// A run as the backend emitted it.
struct TextRun {
    family: String,
    weight: u32,
    italic: bool,
    chars: BTreeSet<char>,
}

/// A per-document font plan: face rules plus the family chains runs paint with.
struct FontPlan {
    families: String,
    serif_chain: String,
    mono_chain: String,
}

/// Embed the bundled body faces into an SVG payload and name them on every run.
///
/// The renderer tags prose runs `font-family="serif"` and code runs
/// `font-family="monospace"`. Both tags are rewritten to the bundled family
/// names, and a `<style>` block carrying a per-document subset of each face is
/// inserted after the document title. A face whose character set is empty is
/// left out, so a document without code does not carry the mono face.
pub fn embed_body_fonts(svg: &str, theme: Theme) -> String {
    // Themes whose layout pass marks heading runs bold carry the weight in the
    // run style. Only the legacy traditional preset still needs the size-based
    // promotion, and it must keep its old output.
    let emphasized = if theme == Theme::Traditional { emphasize_headings(svg, theme) } else { svg.to_string() };
    let runs = text_runs(&emphasized);
    let plan = build_plan(theme, runs);
    let with_style = insert_style(&emphasized, &plan.families, "after-title");
    apply_chains(with_style, &plan)
}

/// Embed the bundled body faces into an HTML payload and name them on every run.
///
/// Reflow and snapshot HTML carry the same generic families as SVG. The style
/// block goes into the document head, and every inline `font-family:serif` or
/// `font-family:monospace` declaration is rewritten to the bundled chain.
pub fn embed_html_fonts(html: &str, theme: Theme) -> String {
    let runs = html_runs(html);
    let plan = build_plan(theme, runs);
    let with_style = insert_style(html, &plan.families, "after-head");
    let mut out = with_style;
    if !plan.serif_chain.is_empty() {
        out = out.replace("font-family:serif", &format!("font-family:'{}'", plan.serif_chain.replace(", ", "','")));
        out = out.replace("font-family: serif", &format!("font-family: '{}'", plan.serif_chain.replace(", ", "','")));
    }
    if !plan.mono_chain.is_empty() {
        out = out.replace("font-family:monospace", &format!("font-family:'{}'", plan.mono_chain.replace(", ", "','")));
        out = out.replace("font-family: monospace", &format!("font-family: '{}'", plan.mono_chain.replace(", ", "','")));
    }
    out
}

/// Collect every painted character set into one face plan.
fn build_plan(theme: Theme, runs: Vec<TextRun>) -> FontPlan {
    let mut sets: Vec<BTreeSet<char>> = vec![BTreeSet::new(); body_faces().len()];
    let mut fallback_needed = false;
    for run in runs {
        let bold = run.weight >= 600;
        let style = if bold {
            BodyFaceStyle::Bold
        }
        else if run.italic {
            BodyFaceStyle::Italic
        }
        else {
            BodyFaceStyle::Regular
        };
        let family = if run.family == "monospace" { BodyFamily::Mono } else { BodyFamily::Serif };
        let style_face =
            body_faces().iter().any(|face| face.family == family && face.style == style && face.script != BodyScript::Cjk);
        let mut remaining = run.chars;
        for index in candidates(theme, &run.family, bold, run.italic) {
            if remaining.is_empty() {
                break;
            }
            let covered = mapped(&body_faces()[index].bytes, &remaining);
            if style_face && !covered.is_empty() && body_faces()[index].style != style {
                fallback_needed = true;
            }
            remaining.retain(|ch| !covered.contains(ch));
            sets[index].extend(covered);
        }
    }

    // Subset each painted face once and remember the bytes, so a style face and
    // the fallback face that repeats it do not encode the payload twice.
    let mut emitted: Vec<(usize, Vec<u8>)> = Vec::new();
    let mut families = String::new();
    for (index, chars) in sets.iter().enumerate() {
        if chars.is_empty() {
            continue;
        }
        let face = &body_faces()[index];
        let bytes = match subset_font(&face.bytes, chars) {
            Some(FacePayload::Subset(subset)) => subset.bytes,
            Some(FacePayload::Full) => face.bytes.to_vec(),
            None => continue,
        };
        families.push_str(&face_rule(face_family(theme, face), face, &bytes));
        emitted.push((index, bytes));
    }

    let serif = emitted.iter().any(|(index, _)| body_faces()[*index].family == BodyFamily::Serif);
    let mono = emitted.iter().any(|(index, _)| body_faces()[*index].family == BodyFamily::Mono);

    // A bold or italic run that had to hand characters to the regular face still
    // paints those characters with the same family, because CSS may match the
    // regular face for a missing bold glyph only through the family list. When
    // that handoff happened, the list ends with bundled regular faces so a
    // missing glyph never reaches a host face.
    let mut fallback = Vec::new();
    if fallback_needed {
        for (index, bytes) in &emitted {
            let face = &body_faces()[*index];
            if face.family != BodyFamily::Serif || face.style != BodyFaceStyle::Regular {
                continue;
            }
            let name = match theme {
                Theme::Baseline | Theme::Editorial | Theme::Classic => FALLBACK_FAMILY,
                Theme::Traditional if face.script == BodyScript::Latin => FALLBACK_FAMILY,
                Theme::Traditional => FALLBACK_CJK_FAMILY,
            };
            families.push_str(&face_rule(name, face, bytes));
            fallback.push(name);
        }
    }

    let (serif_chain, mono_chain) = match theme {
        Theme::Baseline | Theme::Editorial | Theme::Classic => {
            let mut serif_chain = Vec::new();
            if serif {
                serif_chain.push(SERIF_FAMILY);
                serif_chain.extend(fallback.iter().copied());
            }
            let mut mono_chain = Vec::new();
            if mono {
                mono_chain.push(MONO_FAMILY);
            }
            if serif {
                mono_chain.push(SERIF_FAMILY);
                mono_chain.extend(fallback.iter().copied());
            }
            (serif_chain.join(", "), mono_chain.join(", "))
        }
        Theme::Traditional => {
            let mut serif_chain = Vec::new();
            if serif {
                serif_chain.push(SERIF_FAMILY);
            }
            if cjk_faces(&sets) {
                serif_chain.push(CJK_FAMILY);
            }
            serif_chain.extend(fallback.iter().copied());
            let mut mono_chain = Vec::new();
            if mono {
                mono_chain.push(MONO_FAMILY);
            }
            if cjk_faces(&sets) {
                mono_chain.push(CJK_FAMILY);
            }
            if serif {
                mono_chain.push(SERIF_FAMILY);
            }
            mono_chain.extend(fallback.iter().copied());
            (serif_chain.join(", "), mono_chain.join(", "))
        }
    };

    FontPlan { families, serif_chain, mono_chain }
}

fn cjk_faces(sets: &[BTreeSet<char>]) -> bool {
    body_faces().iter().zip(sets.iter()).any(|(face, chars)| face.script == BodyScript::Cjk && !chars.is_empty())
}

/// Read every SVG `<text>` run with its style and its decoded characters.
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
        let Some(tag_end) = svg[start..].find('>').map(|index| start + index)
        else {
            break;
        };
        let Some(close) = svg[tag_end..].find("</text>").map(|index| tag_end + index)
        else {
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

/// Read every visible HTML text node into one serif run.
///
/// HTML runs do not carry per-run style at the text-node level, so the whole
/// body is treated as one prose run. Code spans still need their mono face, so
/// a second run gathers the text inside elements whose style names monospace.
fn html_runs(html: &str) -> Vec<TextRun> {
    let body = html_body(html);
    let mut prose = TextRun { family: "serif".to_string(), weight: 400, italic: false, chars: BTreeSet::new() };
    let mut code = TextRun { family: "monospace".to_string(), weight: 400, italic: false, chars: BTreeSet::new() };
    let mut stack: Vec<bool> = Vec::new();
    let mut cursor = 0usize;
    while let Some(found) = body[cursor..].find('<') {
        let start = cursor + found;
        let text = decode_entities(&body[cursor..start]);
        if stack.iter().any(|mono| *mono) {
            code.chars.extend(text.chars());
        }
        else {
            prose.chars.extend(text.chars());
        }
        let Some(close) = body[start..].find('>').map(|index| start + index)
        else {
            break;
        };
        let tag = &body[start..=close];
        if tag.starts_with("</") {
            stack.pop();
        }
        else if !tag.trim_end_matches('>').trim_end().ends_with('/') {
            let mono = tag.contains("font-family:monospace") || tag.contains("font-family: monospace");
            stack.push(mono);
        }
        cursor = close + 1;
    }
    let tail = decode_entities(&body[cursor..]);
    if stack.iter().any(|mono| *mono) {
        code.chars.extend(tail.chars());
    }
    else {
        prose.chars.extend(tail.chars());
    }
    vec![prose, code]
}

fn html_body(html: &str) -> &str {
    let lower = html.to_ascii_lowercase();
    let start = lower.find("<body").and_then(|index| html[index..].find('>').map(|end| index + end + 1));
    let end = lower.rfind("</body>").or_else(|| lower.rfind("</html>"));
    match (start, end) {
        (Some(start), Some(end)) if end > start => &html[start..end],
        _ => html,
    }
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
    text.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

fn face_index(family: BodyFamily, script: BodyScript, style: BodyFaceStyle) -> Option<usize> {
    body_faces().iter().position(|face| face.family == family && face.script == script && face.style == style)
}

/// Faces a run consults, in the order CSS font matching picks them.
///
/// Under the traditional theme the style face comes first, then its family's
/// regular face, then the Han faces in the chain the rewritten family list
/// declares. Under the baseline theme every prose run paints with Noto Serif
/// SC, whose file carries both Latin and Han outlines, so no cross-family
/// handoff is needed.
fn candidates(theme: Theme, family: &str, bold: bool, italic: bool) -> Vec<usize> {
    let mono = family == "monospace";
    let mut order = Vec::new();
    match theme {
        Theme::Baseline | Theme::Editorial | Theme::Classic => {
            if mono {
                if let Some(index) = face_index(BodyFamily::Mono, BodyScript::Latin, BodyFaceStyle::Regular) {
                    order.push(index);
                }
                if let Some(index) = face_index(BodyFamily::Serif, BodyScript::Cjk, BodyFaceStyle::Regular) {
                    order.push(index);
                }
            }
            else {
                if bold {
                    if let Some(index) = face_index(BodyFamily::Serif, BodyScript::Cjk, BodyFaceStyle::Bold) {
                        order.push(index);
                    }
                }
                if let Some(index) = face_index(BodyFamily::Serif, BodyScript::Cjk, BodyFaceStyle::Regular) {
                    order.push(index);
                }
            }
        }
        Theme::Traditional => {
            let mut primary = Vec::new();
            if mono {
                primary.push((BodyFamily::Mono, BodyScript::Latin, BodyFaceStyle::Regular));
            }
            else {
                if bold {
                    primary.push((BodyFamily::Serif, BodyScript::Latin, BodyFaceStyle::Bold));
                }
                else if italic {
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
        }
    }
    order
}

/// CSS family a bundled face is published under in a theme.
fn face_family(theme: Theme, face: &BodyFace) -> &'static str {
    match theme {
        Theme::Baseline | Theme::Editorial | Theme::Classic => match face.family {
            BodyFamily::Serif => SERIF_FAMILY,
            BodyFamily::Mono => MONO_FAMILY,
        },
        Theme::Traditional => match (face.family, face.script) {
            (BodyFamily::Serif, BodyScript::Latin) => SERIF_FAMILY,
            (BodyFamily::Serif, BodyScript::Cjk) => CJK_FAMILY,
            (BodyFamily::Mono, _) => MONO_FAMILY,
        },
    }
}

/// One `@font-face` rule for a bundled face, carrying the payload as a data URI.
fn face_rule(family: &str, face: &BodyFace, bytes: &[u8]) -> String {
    let style = if face.style == BodyFaceStyle::Italic { "italic" } else { "normal" };
    let weight = if face.style == BodyFaceStyle::Bold { 700 } else { 400 };
    let data = encode_base64(bytes);
    format!(
        "@font-face{{font-family:'{family}';font-style:{style};font-weight:{weight};\
         src:url(data:font/ttf;base64,{data}) format('truetype');}}"
    )
}

/// Mark heading runs bold so headings separate from body text structurally.
///
/// `document-layout` scales a heading's size but has no weight axis, so without
/// this pass a heading differs from body text only by size. The threshold is
/// the body size plus a small allowance, which no body or code run exceeds.
fn emphasize_headings(svg: &str, theme: Theme) -> String {
    let threshold = theme.heading_weight_threshold();
    let mut out = String::with_capacity(svg.len());
    let mut cursor = 0usize;
    while let Some(found) = svg[cursor..].find("<text ") {
        let start = cursor + found;
        out.push_str(&svg[cursor..start]);
        let Some(end) = svg[start..].find('>').map(|index| start + index)
        else {
            out.push_str(&svg[start..]);
            return out;
        };
        let tag = &svg[start..=end];
        let size = attribute(tag, "font-size")
            .and_then(|value| value.trim_end_matches("px").trim_end_matches("pt").parse::<f64>().ok())
            .unwrap_or(0.0);
        if size >= threshold && !tag.contains("font-weight=") {
            out.push_str(&tag[..tag.len() - 1]);
            out.push_str(" font-weight=\"700\">");
        }
        else {
            out.push_str(tag);
        }
        cursor = end + 1;
    }
    out.push_str(&svg[cursor..]);
    out
}

fn insert_style(document: &str, families: &str, placement: &str) -> String {
    let style = format!("<style type=\"text/css\">{families}</style>");
    if families.is_empty() {
        return document.to_string();
    }
    match placement {
        "after-title" => match document.find("</title>") {
            Some(index) => {
                let split = index + "</title>".len();
                format!("{}{}{}", &document[..split], style, &document[split..])
            }
            None => match document.find('>') {
                Some(index) => format!("{}{}{}", &document[..=index], style, &document[index + 1..]),
                None => document.to_string(),
            },
        },
        _ => {
            let lower = document.to_ascii_lowercase();
            if let Some(index) = lower.find("<head") {
                if let Some(end) = document[index..].find('>').map(|offset| index + offset) {
                    return format!("{}{}{}", &document[..=end], style, &document[end + 1..]);
                }
            }
            if let Some(index) = lower.find("<html") {
                if let Some(end) = document[index..].find('>').map(|offset| index + offset) {
                    return format!("{}{}{}", &document[..=end], style, &document[end + 1..]);
                }
            }
            format!("{style}{document}")
        }
    }
}

fn apply_chains(mut out: String, plan: &FontPlan) -> String {
    if !plan.serif_chain.is_empty() {
        out = out.replace("font-family=\"serif\"", &format!("font-family=\"{}\"", plan.serif_chain));
    }
    if !plan.mono_chain.is_empty() {
        out = out.replace("font-family=\"monospace\"", &format!("font-family=\"{}\"", plan.mono_chain));
    }
    out
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
    fn latin_prose_carries_one_bundled_face_and_no_generic_family() {
        let svg = r#"<svg><title>x</title><text font-family="serif" font-size="11">Hello world.</text></svg>"#;
        let out = embed_body_fonts(svg, Theme::Baseline);
        assert!(out.contains("@font-face"));
        assert!(out.contains("font-family=\"VuTeX Serif\""));
        assert!(!out.contains("font-family=\"serif\""));
        assert!(!out.contains("font-family=\"monospace\""));
    }

    #[test]
    fn baseline_publishes_a_bold_face_for_the_weight_the_layout_emitted() {
        let svg = r#"<svg><text font-family="serif" font-weight="700" font-size="17.6">Title</text><text font-family="serif" font-size="11">Body</text></svg>"#;
        let out = embed_body_fonts(svg, Theme::Baseline);
        assert!(out.contains("font-weight=\"700\""), "heading run lost its weight");
        assert!(!out.contains("font-size=\"11\" font-weight=\"700\""));
        assert!(out.contains("font-weight:700"), "heading run got no bold face");
    }

    #[test]
    fn baseline_does_not_guess_heading_weight_from_size() {
        let svg = r#"<svg><text font-family="serif" font-size="17.6">Title</text><text font-family="serif" font-size="11">Body</text></svg>"#;
        let out = embed_body_fonts(svg, Theme::Baseline);
        assert!(!out.contains("font-weight=\"700\""), "the emitter re-guessed the heading weight");
    }

    #[test]
    fn traditional_promotes_heading_runs_by_size() {
        let svg = r#"<svg><text font-family="serif" font-size="22">Chapter One</text><text font-family="serif" font-size="11">Body text</text></svg>"#;
        let out = embed_body_fonts(svg, Theme::Traditional);
        assert!(out.contains("font-size=\"22\" font-weight=\"700\""), "heading run got no weight");
        assert!(out.contains("font-weight:700"), "heading run got no bold face");
    }

    #[test]
    fn a_style_face_that_defers_a_glyph_gets_a_bundled_fallback_tail() {
        let svg = r#"<svg><text font-family="serif" font-style="italic" font-size="11">汉字</text></svg>"#;
        let out = embed_body_fonts(svg, Theme::Traditional);
        assert!(out.contains("@font-face{font-family:'VuTeX Fallback CJK'"), "no bundled fallback face");
        assert!(out.contains("VuTeX Fallback CJK\""), "chain has no fallback tail");
    }

    #[test]
    fn html_payloads_carry_the_same_face_rules() {
        let html = r#"<html><head></head><body><div style="font-family:serif">Hello</div></body></html>"#;
        let out = embed_html_fonts(html, Theme::Baseline);
        assert!(out.contains("@font-face"));
        assert!(out.contains("font-family:'VuTeX Serif'"));
        assert!(!out.contains("font-family:serif"));
    }
}
