//! Per-document TrueType subsetting for the embedded SVG body faces.
//!
//! Subsetting keeps every original glyph id, so the tables that address glyphs
//! by id (`GSUB`, `GPOS`, `GDEF`, `hmtx`, `vmtx`, `post`) stay valid without a
//! rewrite. Glyphs that the document never paints are replaced by empty
//! entries, and `cmap` is rebuilt to map only the characters the document uses.
//! Because ids do not move, a shaper picks the same glyphs for the same text
//! before and after the subset, which is why the painted raster stays
//! pixel-identical to the full face.
//!
//! A face that carries OpenType substitutions (`GSUB` features) could compose
//! glyphs the character set does not name, so such faces refuse to subset and
//! are embedded whole. That is a deliberate full-face exception rather than a
//! silent miss: the caller still embeds a working face.

use std::collections::{BTreeMap, BTreeSet};

/// One subset face with the counts the report needs.
#[derive(Debug, Clone)]
pub struct SubsetFont {
    /// Complete sfnt payload for the subset.
    pub bytes: Vec<u8>,
    /// Non-empty glyphs kept in `glyf`, including the composite dependencies.
    pub glyph_count: usize,
    /// Characters the rebuilt `cmap` maps.
    pub codepoint_count: usize,
}

/// Whether a face was subset or had to be embedded whole.
#[derive(Debug, Clone)]
pub enum FacePayload {
    /// A document-specific subset.
    Subset(SubsetFont),
    /// The original face, kept because subsetting it could drop a glyph that a
    /// shaping feature would compose.
    Full,
}

/// Why a face produced no subset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Refusal {
    /// The face maps none of the requested characters, so the document never
    /// paints with it and the caller should leave it out entirely.
    Unmapped,
    /// The face is not safely subsettable; the caller should embed it whole.
    Unsafe,
}

type TableDirectory = BTreeMap<[u8; 4], (usize, usize)>;

/// Subset `font` to the characters in `chars`, preserving glyph ids.
///
/// Returns `None` when the face maps none of `chars`, so a Latin page never
/// carries a CJK face it never paints. Returns [`FacePayload::Full`] when the
/// face is not a `glyf` font or when its `GSUB` declares features, because a
/// substitution could reach a glyph that the character set does not name.
pub fn subset_font(font: &[u8], chars: &BTreeSet<char>) -> Option<FacePayload> {
    match build_subset(font, chars) {
        Ok(subset) => Some(FacePayload::Subset(subset)),
        Err(Refusal::Unmapped) => None,
        Err(Refusal::Unsafe) => Some(FacePayload::Full),
    }
}

/// Return the characters in `chars` that `font` maps.
pub fn mapped(font: &[u8], chars: &BTreeSet<char>) -> BTreeSet<char> {
    let Some(directory) = parse_directory(font)
    else {
        return BTreeSet::new();
    };
    let Some(cmap) = table(font, &directory, b"cmap")
    else {
        return BTreeSet::new();
    };
    chars.iter().copied().filter(|ch| glyph_for(cmap, *ch as u32).is_some()).collect()
}

fn build_subset(font: &[u8], chars: &BTreeSet<char>) -> Result<SubsetFont, Refusal> {
    let directory = parse_directory(font).ok_or(Refusal::Unsafe)?;
    let head = table(font, &directory, b"head").ok_or(Refusal::Unsafe)?;
    let maxp = table(font, &directory, b"maxp").ok_or(Refusal::Unsafe)?;
    let loca = table(font, &directory, b"loca").ok_or(Refusal::Unsafe)?;
    let glyf = table(font, &directory, b"glyf").ok_or(Refusal::Unsafe)?;
    let cmap = table(font, &directory, b"cmap").ok_or(Refusal::Unsafe)?;

    if head.len() < 54 || maxp.len() < 6 {
        return Err(Refusal::Unsafe);
    }
    if declares_substitutions(font, &directory) {
        return Err(Refusal::Unsafe);
    }

    let num_glyphs = read_u16(maxp, 4).ok_or(Refusal::Unsafe)? as usize;
    let long_loca = read_u16(head, 50).ok_or(Refusal::Unsafe)? != 0;
    let offsets = read_loca(loca, num_glyphs, long_loca).ok_or(Refusal::Unsafe)?;

    let mut retained = BTreeSet::from([0u16]);
    let mut mapped = Vec::new();
    for ch in chars {
        let codepoint = *ch as u32;
        if let Some(glyph) = glyph_for(cmap, codepoint) {
            if (glyph as usize) < num_glyphs {
                retained.insert(glyph);
                mapped.push((codepoint, glyph));
            }
        }
    }
    if mapped.is_empty() {
        return Err(Refusal::Unmapped);
    }

    retain_composites(glyf, &offsets, &mut retained);

    let (new_glyf, new_offsets) = rebuild_glyf(glyf, &offsets, num_glyphs, &retained);

    let mut new_head = head.to_vec();
    write_u16(&mut new_head, 50, 1);
    write_u32(&mut new_head, 8, 0);

    mapped.sort_unstable_by_key(|(codepoint, _)| *codepoint);
    let new_cmap = rebuild_cmap(&mapped);
    let new_loca = rebuild_loca(&new_offsets);

    let mut replacements =
        BTreeMap::from([(*b"head", new_head), (*b"glyf", new_glyf), (*b"loca", new_loca), (*b"cmap", new_cmap)]);

    let bytes = assemble(font, &directory, &mut replacements).ok_or(Refusal::Unsafe)?;
    let glyph_count = retained.iter().filter(|glyph| offsets[**glyph as usize] < offsets[**glyph as usize + 1]).count();
    Ok(SubsetFont { bytes, glyph_count, codepoint_count: mapped.len() })
}

fn parse_directory(font: &[u8]) -> Option<TableDirectory> {
    if font.len() < 12 {
        return None;
    }
    let count = read_u16(font, 4)? as usize;
    let mut directory = BTreeMap::new();
    for index in 0..count {
        let record = 12 + index * 16;
        let tag: [u8; 4] = font.get(record..record + 4)?.try_into().ok()?;
        let offset = read_u32(font, record + 8)? as usize;
        let length = read_u32(font, record + 12)? as usize;
        if offset.checked_add(length)? > font.len() {
            return None;
        }
        directory.insert(tag, (offset, length));
    }
    Some(directory)
}

fn table<'a>(font: &'a [u8], directory: &TableDirectory, tag: &[u8; 4]) -> Option<&'a [u8]> {
    let (offset, length) = *directory.get(tag)?;
    font.get(offset..offset + length)
}

/// Reject faces whose shaping layer maps one glyph to another, because the
/// replacement glyph may not be in the character set.
fn declares_substitutions(font: &[u8], directory: &TableDirectory) -> bool {
    let Some(gsub) = table(font, directory, b"GSUB")
    else {
        return false;
    };
    if gsub.len() < 10 {
        return true;
    }
    let feature_list = read_u16(gsub, 6).unwrap_or(0) as usize;
    let Some(header) = gsub.get(feature_list..feature_list + 2)
    else {
        return true;
    };
    read_u16(header, 0).unwrap_or(1) > 0
}

fn read_loca(loca: &[u8], num_glyphs: usize, long_format: bool) -> Option<Vec<usize>> {
    let width = if long_format { 4 } else { 2 };
    if loca.len() < (num_glyphs + 1) * width {
        return None;
    }
    let mut offsets = Vec::with_capacity(num_glyphs + 1);
    for index in 0..=num_glyphs {
        let value = if long_format { read_u32(loca, index * 4)? as usize } else { read_u16(loca, index * 2)? as usize * 2 };
        offsets.push(value);
    }
    Some(offsets)
}

fn rebuild_loca(offsets: &[usize]) -> Vec<u8> {
    let mut out = Vec::with_capacity(offsets.len() * 4);
    for offset in offsets {
        out.extend_from_slice(&(*offset as u32).to_be_bytes());
    }
    out
}

fn glyph_for(cmap: &[u8], codepoint: u32) -> Option<u16> {
    let subtable = select_cmap(cmap)?;
    match read_u16(cmap, subtable)? {
        4 => lookup_format4(cmap, subtable, codepoint),
        12 => lookup_format12(cmap, subtable, codepoint),
        _ => None,
    }
}

fn select_cmap(cmap: &[u8]) -> Option<usize> {
    let count = read_u16(cmap, 2)? as usize;
    let mut best = None;
    let mut best_score = 0;
    for index in 0..count {
        let record = 4 + index * 8;
        let platform = read_u16(cmap, record)?;
        let encoding = read_u16(cmap, record + 2)?;
        let offset = read_u32(cmap, record + 4)? as usize;
        let format = read_u16(cmap, offset)?;
        let score = match (platform, encoding, format) {
            (3, 10, 12) | (0, 4, 12) | (0, 6, 12) => 4,
            (3, 1, 4) | (0, 3, 4) => 3,
            (_, _, 12) => 2,
            (_, _, 4) => 1,
            _ => 0,
        };
        if score > best_score {
            best_score = score;
            best = Some(offset);
        }
    }
    best
}

fn lookup_format4(cmap: &[u8], subtable: usize, codepoint: u32) -> Option<u16> {
    if codepoint > u32::from(u16::MAX) {
        return None;
    }
    let codepoint = codepoint as u16;
    let segment_count = read_u16(cmap, subtable + 6)? as usize / 2;
    let end_codes = subtable + 14;
    let start_codes = end_codes + segment_count * 2 + 2;
    let deltas = start_codes + segment_count * 2;
    let range_offsets = deltas + segment_count * 2;
    for segment in 0..segment_count {
        let end = read_u16(cmap, end_codes + segment * 2)?;
        if end < codepoint {
            continue;
        }
        let start = read_u16(cmap, start_codes + segment * 2)?;
        if start > codepoint {
            return None;
        }
        let delta = read_u16(cmap, deltas + segment * 2)?;
        let range_offset = read_u16(cmap, range_offsets + segment * 2)?;
        if range_offset == 0 {
            return Some(codepoint.wrapping_add(delta));
        }
        let glyph_at = range_offsets + segment * 2 + range_offset as usize + 2 * (codepoint - start) as usize;
        let glyph = read_u16(cmap, glyph_at)?;
        if glyph == 0 {
            return None;
        }
        return Some(glyph.wrapping_add(delta));
    }
    None
}

fn lookup_format12(cmap: &[u8], subtable: usize, codepoint: u32) -> Option<u16> {
    let group_count = read_u32(cmap, subtable + 12)? as usize;
    let groups = subtable + 16;
    for group in 0..group_count {
        let record = groups + group * 12;
        let start = read_u32(cmap, record)?;
        let end = read_u32(cmap, record + 4)?;
        if codepoint < start || codepoint > end {
            continue;
        }
        let glyph = read_u32(cmap, record + 8)?;
        return u16::try_from(glyph.checked_add(codepoint - start)?).ok();
    }
    None
}

fn retain_composites(glyf: &[u8], offsets: &[usize], retained: &mut BTreeSet<u16>) {
    loop {
        let mut added = false;
        let pending: Vec<u16> = retained.iter().copied().collect();
        for glyph in pending {
            let start = offsets[glyph as usize];
            let end = offsets[glyph as usize + 1];
            if end <= start + 10 {
                continue;
            }
            let contours = i16::from_be_bytes([glyf[start], glyf[start + 1]]);
            if contours >= 0 {
                continue;
            }
            let mut cursor = start + 10;
            loop {
                if cursor + 4 > end {
                    break;
                }
                let Some(flags) = read_u16(glyf, cursor)
                else {
                    break;
                };
                let Some(component) = read_u16(glyf, cursor + 2)
                else {
                    break;
                };
                cursor += 4;
                cursor += if flags & 0x0001 != 0 { 4 } else { 2 };
                if flags & 0x0008 != 0 {
                    cursor += 2;
                }
                else if flags & 0x0040 != 0 {
                    cursor += 4;
                }
                else if flags & 0x0080 != 0 {
                    cursor += 8;
                }
                if (component as usize) < offsets.len() - 1 && retained.insert(component) {
                    added = true;
                }
                if flags & 0x0020 == 0 {
                    break;
                }
            }
        }
        if !added {
            return;
        }
    }
}

fn rebuild_glyf(glyf: &[u8], offsets: &[usize], num_glyphs: usize, retained: &BTreeSet<u16>) -> (Vec<u8>, Vec<usize>) {
    let mut out = Vec::with_capacity(glyf.len());
    let mut new_offsets = Vec::with_capacity(num_glyphs + 1);
    for glyph in 0..num_glyphs {
        new_offsets.push(out.len());
        if !retained.contains(&(glyph as u16)) {
            continue;
        }
        let start = offsets[glyph];
        let end = offsets[glyph + 1];
        if end <= start {
            continue;
        }
        out.extend_from_slice(&glyf[start..end]);
        while out.len() % 4 != 0 {
            out.push(0);
        }
    }
    new_offsets.push(out.len());
    (out, new_offsets)
}

fn rebuild_cmap(mappings: &[(u32, u16)]) -> Vec<u8> {
    let bmp: Vec<(u32, u16)> = mappings.iter().copied().filter(|(codepoint, _)| *codepoint < 0xFFFF).collect();
    let format4 = build_format4(&bmp);
    let format12 = build_format12(mappings);

    let header_size = 4 + 8 * 2;
    let format4_offset = header_size;
    let format12_offset = format4_offset + format4.len();

    let mut out = Vec::with_capacity(format12_offset + format12.len());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&2u16.to_be_bytes());
    out.extend_from_slice(&3u16.to_be_bytes());
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&(format4_offset as u32).to_be_bytes());
    out.extend_from_slice(&3u16.to_be_bytes());
    out.extend_from_slice(&10u16.to_be_bytes());
    out.extend_from_slice(&(format12_offset as u32).to_be_bytes());
    out.extend_from_slice(&format4);
    out.extend_from_slice(&format12);
    out
}

fn build_format4(mappings: &[(u32, u16)]) -> Vec<u8> {
    let segment_count = mappings.len() + 1;
    let search_range = 2 * 2usize.pow((segment_count as u32).ilog2());
    let entry_selector = (segment_count as u32).ilog2() as u16;
    let range_shift = segment_count * 2 - search_range;
    let length = 16 + segment_count * 8;

    let mut out = Vec::with_capacity(length);
    out.extend_from_slice(&4u16.to_be_bytes());
    out.extend_from_slice(&(length as u16).to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&((segment_count * 2) as u16).to_be_bytes());
    out.extend_from_slice(&(search_range as u16).to_be_bytes());
    out.extend_from_slice(&entry_selector.to_be_bytes());
    out.extend_from_slice(&(range_shift as u16).to_be_bytes());
    for (codepoint, _) in mappings {
        out.extend_from_slice(&(*codepoint as u16).to_be_bytes());
    }
    out.extend_from_slice(&0xFFFFu16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    for (codepoint, _) in mappings {
        out.extend_from_slice(&(*codepoint as u16).to_be_bytes());
    }
    out.extend_from_slice(&0xFFFFu16.to_be_bytes());
    for (codepoint, glyph) in mappings {
        let delta = (i32::from(*glyph) - *codepoint as i32) as u16;
        out.extend_from_slice(&delta.to_be_bytes());
    }
    out.extend_from_slice(&1u16.to_be_bytes());
    for _ in mappings {
        out.extend_from_slice(&0u16.to_be_bytes());
    }
    out.extend_from_slice(&0u16.to_be_bytes());
    out
}

fn build_format12(mappings: &[(u32, u16)]) -> Vec<u8> {
    let mut groups: Vec<(u32, u32, u32)> = Vec::new();
    for (codepoint, glyph) in mappings {
        match groups.last_mut() {
            Some((start, end, start_glyph))
                if *end + 1 == *codepoint && *start_glyph + (*end - *start) + 1 == u32::from(*glyph) =>
            {
                *end = *codepoint;
            }
            _ => groups.push((*codepoint, *codepoint, u32::from(*glyph))),
        }
    }
    let length = 16 + groups.len() * 12;
    let mut out = Vec::with_capacity(length);
    out.extend_from_slice(&12u16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&(length as u32).to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(&(groups.len() as u32).to_be_bytes());
    for (start, end, start_glyph) in groups {
        out.extend_from_slice(&start.to_be_bytes());
        out.extend_from_slice(&end.to_be_bytes());
        out.extend_from_slice(&start_glyph.to_be_bytes());
    }
    out
}

fn assemble(font: &[u8], directory: &TableDirectory, replacements: &mut BTreeMap<[u8; 4], Vec<u8>>) -> Option<Vec<u8>> {
    let mut tables: Vec<([u8; 4], Vec<u8>)> = Vec::new();
    for (tag, (offset, length)) in directory {
        if tag == b"DSIG" {
            continue;
        }
        let bytes = match replacements.remove(tag) {
            Some(bytes) => bytes,
            None => font.get(*offset..*offset + *length)?.to_vec(),
        };
        tables.push((*tag, bytes));
    }
    for (tag, bytes) in replacements.iter() {
        tables.push((*tag, bytes.clone()));
    }
    tables.sort_by(|left, right| left.0.cmp(&right.0));

    let count = tables.len();
    let search_range = 16 * 2usize.pow((count as u32).ilog2());
    let entry_selector = (count as u32).ilog2() as u16;
    let range_shift = count * 16 - search_range;

    let header_size = 12 + count * 16;
    let mut out = vec![0u8; header_size];
    write_u32(&mut out, 0, 0x0001_0000);
    write_u16(&mut out, 4, count as u16);
    write_u16(&mut out, 6, search_range as u16);
    write_u16(&mut out, 8, entry_selector);
    write_u16(&mut out, 10, range_shift as u16);

    let mut head_offset = 0usize;
    for (index, (tag, bytes)) in tables.iter().enumerate() {
        let record = 12 + index * 16;
        while out.len() % 4 != 0 {
            out.push(0);
        }
        let offset = out.len();
        if tag == b"head" {
            head_offset = offset;
        }
        out.extend_from_slice(bytes);
        while out.len() % 4 != 0 {
            out.push(0);
        }
        let checksum = table_checksum(&out[offset..out.len()]);
        out[record..record + 4].copy_from_slice(tag);
        out[record + 4..record + 8].copy_from_slice(&checksum.to_be_bytes());
        out[record + 8..record + 12].copy_from_slice(&(offset as u32).to_be_bytes());
        out[record + 12..record + 16].copy_from_slice(&(bytes.len() as u32).to_be_bytes());
    }

    let total = table_checksum(&out);
    let adjustment = 0xB1B0_AFBAu32.wrapping_sub(total);
    out.get_mut(head_offset + 8..head_offset + 12)?.copy_from_slice(&adjustment.to_be_bytes());
    Some(out)
}

fn table_checksum(bytes: &[u8]) -> u32 {
    let mut sum = 0u32;
    for chunk in bytes.chunks(4) {
        let mut word = [0u8; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        sum = sum.wrapping_add(u32::from_be_bytes(word));
    }
    sum
}

fn read_u16(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(data.get(at..at + 2)?.try_into().ok()?))
}

fn read_u32(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

fn write_u16(data: &mut [u8], at: usize, value: u16) {
    data[at..at + 2].copy_from_slice(&value.to_be_bytes());
}

fn write_u32(data: &mut [u8], at: usize, value: u32) {
    data[at..at + 4].copy_from_slice(&value.to_be_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use document_fonts::{BodyFaceStyle, BodyFamily, BodyScript};

    fn face(family: BodyFamily, script: BodyScript, style: BodyFaceStyle) -> &'static [u8] {
        crate::assets::configure_test_fonts();
        crate::assets::body_faces()
            .iter()
            .find(|face| face.family == family && face.script == script && face.style == style)
            .expect("packed face")
            .bytes
            .as_ref()
    }

    fn glyph_body<'a>(font: &'a [u8], directory: &TableDirectory, glyph: u16) -> &'a [u8] {
        let head = table(font, directory, b"head").expect("head");
        let maxp = table(font, directory, b"maxp").expect("maxp");
        let loca = table(font, directory, b"loca").expect("loca");
        let glyf = table(font, directory, b"glyf").expect("glyf");
        let count = read_u16(maxp, 4).expect("numGlyphs") as usize;
        let offsets = read_loca(loca, count, read_u16(head, 50).expect("loca format") != 0).expect("loca");
        &glyf[offsets[glyph as usize]..offsets[glyph as usize + 1]]
    }

    #[test]
    fn ten_characters_shrink_tinos_by_more_than_five_times() {
        let source = face(BodyFamily::Serif, BodyScript::Latin, BodyFaceStyle::Regular);
        let chars: BTreeSet<char> = "abcABC123".chars().collect();
        let Some(FacePayload::Subset(subset)) = subset_font(source, &chars)
        else {
            panic!("Tinos must subset")
        };
        assert!(subset.bytes.len() * 5 < source.len(), "{} vs {}", subset.bytes.len(), source.len());
        assert_eq!(subset.codepoint_count, chars.len());
        assert!(subset.glyph_count >= chars.len(), "{}", subset.glyph_count);

        let directory = parse_directory(&subset.bytes).expect("subset directory");
        let cmap = table(&subset.bytes, &directory, b"cmap").expect("subset cmap");
        for ch in &chars {
            let glyph = glyph_for(cmap, *ch as u32).expect("mapped character");
            assert!(!glyph_body(&subset.bytes, &directory, glyph).is_empty(), "{ch}");
        }
    }

    #[test]
    fn composite_dependencies_survive_the_subset() {
        let source = face(BodyFamily::Serif, BodyScript::Latin, BodyFaceStyle::Regular);
        let chars: BTreeSet<char> = "é".chars().collect();
        let Some(FacePayload::Subset(subset)) = subset_font(source, &chars)
        else {
            panic!("Tinos must subset")
        };
        let directory = parse_directory(&subset.bytes).expect("subset directory");
        let cmap = table(&subset.bytes, &directory, b"cmap").expect("subset cmap");
        let glyph = glyph_for(cmap, u32::from('é')).expect("mapped character");
        let body = glyph_body(&subset.bytes, &directory, glyph);
        let contours = i16::from_be_bytes([body[0], body[1]]);
        assert!(contours < 0, "the accent must be a composite");
        let mut cursor = 10;
        let mut components = 0;
        loop {
            let flags = read_u16(body, cursor).expect("component flags");
            let component = read_u16(body, cursor + 2).expect("component glyph");
            assert!(!glyph_body(&subset.bytes, &directory, component).is_empty(), "component {component} was dropped");
            components += 1;
            cursor += 4;
            cursor += if flags & 0x0001 != 0 { 4 } else { 2 };
            if flags & 0x0008 != 0 {
                cursor += 2;
            }
            else if flags & 0x0040 != 0 {
                cursor += 4;
            }
            else if flags & 0x0080 != 0 {
                cursor += 8;
            }
            if flags & 0x0020 == 0 {
                break;
            }
        }
        assert!(components >= 2, "expected base plus accent, saw {components}");
    }

    #[test]
    fn every_packed_face_subset_safely() {
        crate::assets::configure_test_fonts();
        for face in crate::assets::body_faces() {
            let directory = parse_directory(&face.bytes).expect("packed directory");
            assert!(!declares_substitutions(&face.bytes, &directory), "{:?} declares GSUB features", face.family);
            assert!(table(&face.bytes, &directory, b"glyf").is_some(), "{:?} is not a glyf face", face.family);
        }
    }

    #[test]
    fn empty_character_set_produces_no_payload() {
        let source = face(BodyFamily::Mono, BodyScript::Latin, BodyFaceStyle::Regular);
        let chars = BTreeSet::new();
        assert!(subset_font(source, &chars).is_none());
    }
}
