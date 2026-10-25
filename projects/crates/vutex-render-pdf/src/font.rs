//! System TrueType font discovery and subsetting for PDF text outside WinAnsi.
//!
//! WinAnsi-representable text keeps using the base-14 fonts, so common Latin
//! documents stay small and portable. Anything the encoding cannot reach needs
//! real glyphs, so this module finds a system font with TrueType outlines,
//! subsets it to the characters one document actually uses, and hands the
//! subset to the writer as an Identity-H `CIDFontType2` font program.
//!
//! Only `glyf`-outline fonts are supported. Collections (`.ttc`) contribute
//! their first face. CFF-flavoured fonts (`OTTO`) are skipped, because they
//! need a different embedding format.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

/// Absolute paths probed for a `glyf`-outline font, in preference order.
///
/// Windows, macOS, and Linux installers put their CJK faces in different
/// places, so the list is flat and every entry is simply skipped when absent.
const CANDIDATES: &[&str] = &[
    // Windows: SimHei and DengXian ship single-face TrueType files.
    r"C:\Windows\Fonts\simhei.ttf",
    r"C:\Windows\Fonts\Deng.ttf",
    r"C:\Windows\Fonts\simkai.ttf",
    r"C:\Windows\Fonts\simfang.ttf",
    r"C:\Windows\Fonts\msyh.ttc",
    r"C:\Windows\Fonts\simsun.ttc",
    // macOS keeps a TrueType fallback next to the CFF system faces.
    "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
    "/Library/Fonts/Arial Unicode.ttf",
    // Linux distributions that install Droid Sans Fallback or WenQuanYi.
    "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
    "/usr/share/fonts/truetype/droid/DroidSansFallback.ttf",
    "/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc",
    "/usr/share/fonts/truetype/arphic/uming.ttc",
];

/// Shorthand for a four-byte sfnt table tag.
type Tag = [u8; 4];

/// One parsed `glyf`-outline font.
#[derive(Debug)]
pub(crate) struct SystemFont {
    raw: Vec<u8>,
    tables: BTreeMap<Tag, (usize, usize)>,
    units_per_em: u16,
    num_glyphs: u16,
    loca: Vec<u32>,
    glyph_offset: usize,
    /// Advance width and left side bearing per glyph.
    hmetrics: Vec<(u16, i16)>,
    cmap: Cmap,
    bbox: [i16; 4],
    ascent: i16,
    descent: i16,
    cap_height: i16,
    italic_angle: f64,
}

impl SystemFont {
    /// Load the system font that covers the most characters in `charset`.
    ///
    /// Candidates differ in coverage — a CJK face may lack accented Latin — so
    /// the first font that reaches every character wins, and otherwise the one
    /// with the best coverage is kept.
    pub(crate) fn discover(charset: &BTreeSet<char>) -> Option<Self> {
        let mut best: Option<(usize, Self)> = None;
        for path in CANDIDATES {
            let Some(font) = Self::load(Path::new(path))
            else {
                continue;
            };
            let covered = font.coverage(charset);
            if covered == charset.len() {
                return Some(font);
            }
            if best.as_ref().is_none_or(|(best_covered, _)| covered > *best_covered) {
                best = Some((covered, font));
            }
        }
        best.filter(|(covered, _)| *covered > 0).map(|(_, font)| font)
    }

    /// Number of characters in `charset` this font has a glyph for.
    fn coverage(&self, charset: &BTreeSet<char>) -> usize {
        charset.iter().filter(|ch| self.glyph_id(**ch).is_some()).count()
    }

    /// Characters in `charset` this font has no glyph for, in code point order.
    pub(crate) fn missing(&self, charset: &BTreeSet<char>) -> Vec<char> {
        charset.iter().copied().filter(|ch| self.glyph_id(*ch).is_none()).collect()
    }

    /// Load one font file, rejecting CFF outlines and malformed tables.
    pub(crate) fn load(path: &Path) -> Option<Self> {
        let raw = fs::read(path).ok()?;
        Self::parse(raw)
    }

    fn parse(raw: Vec<u8>) -> Option<Self> {
        let base = face_offset(&raw)?;
        let tables = read_table_directory(&raw, base)?;
        let range = |tag: Tag| tables.get(&tag).copied();
        let head = range(*b"head")?;
        let hhea = range(*b"hhea")?;
        let maxp = range(*b"maxp")?;
        let hmtx = range(*b"hmtx")?;
        let cmap_table = range(*b"cmap")?;
        let loca_table = range(*b"loca")?;
        let glyf = range(*b"glyf")?;

        let units_per_em = u16_at(&raw, head.0 + 18)?;
        if units_per_em == 0 {
            return None;
        }
        let num_glyphs = u16_at(&raw, maxp.0 + 4)?;
        let index_to_loc = i16_at(&raw, head.0 + 50)?;
        let ascender = i16_at(&raw, hhea.0 + 4)?;
        let descender = i16_at(&raw, hhea.0 + 6)?;
        let horizontal_count = u16_at(&raw, hhea.0 + 34)?;

        let loca = read_loca(&raw, loca_table, num_glyphs, index_to_loc)?;
        let hmetrics = read_hmetrics(&raw, hmtx, horizontal_count, num_glyphs)?;
        let cmap = Cmap::parse(&raw, cmap_table.0, cmap_table.1)?;

        let bbox =
            [i16_at(&raw, head.0 + 36)?, i16_at(&raw, head.0 + 38)?, i16_at(&raw, head.0 + 40)?, i16_at(&raw, head.0 + 42)?];
        let cap_height = read_cap_height(&raw, &tables, ascender);
        let italic_angle = read_italic_angle(&raw, &tables);

        Some(Self {
            raw,
            tables,
            units_per_em,
            num_glyphs,
            loca,
            glyph_offset: glyf.0,
            hmetrics,
            cmap,
            bbox,
            ascent: ascender,
            descent: descender,
            cap_height,
            italic_angle,
        })
    }

    /// Glyph id for one character, if the font covers it.
    fn glyph_id(&self, ch: char) -> Option<u16> {
        self.cmap.lookup(ch as u32).filter(|glyph| *glyph < self.num_glyphs)
    }

    /// Raw `glyf` bytes of one glyph.
    fn glyph_data(&self, glyph: u16) -> &[u8] {
        let start = self.loca[usize::from(glyph)];
        let end = self.loca[usize::from(glyph) + 1];
        if start > end {
            return &[];
        }
        self.raw.get(self.glyph_offset + start as usize..self.glyph_offset + end as usize).unwrap_or(&[])
    }

    /// Glyph ids referenced by a composite glyph, in component order.
    fn components(&self, glyph: u16) -> Vec<u16> {
        composite_components(self.glyph_data(glyph))
    }

    /// Build a font program that contains only the glyphs `charset` needs.
    ///
    /// Returns `None` when the font covers none of the characters.
    pub(crate) fn subset(&self, charset: &BTreeSet<char>) -> Option<Subset> {
        // Old glyph id -> characters it was selected for. Glyph 0 (`.notdef`)
        // always stays, and composites pull their components in below.
        let mut keep: BTreeSet<u16> = BTreeSet::from([0]);
        let mut chars: BTreeMap<char, u16> = BTreeMap::new();
        for &ch in charset {
            if let Some(glyph) = self.glyph_id(ch) {
                keep.insert(glyph);
                chars.insert(ch, glyph);
            }
        }
        if chars.is_empty() {
            return None;
        }
        let mut queue: Vec<u16> = keep.iter().copied().collect();
        while let Some(glyph) = queue.pop() {
            for component in self.components(glyph) {
                if keep.insert(component) {
                    queue.push(component);
                }
            }
        }

        let order: Vec<u16> = keep.iter().copied().collect();
        let mapping: BTreeMap<u16, u16> = order.iter().enumerate().map(|(index, old)| (*old, index as u16)).collect();
        let num_glyphs = order.len() as u16;

        let mut glyf = Vec::new();
        let mut loca = Vec::with_capacity(order.len() + 1);
        loca.push(0u32);
        for old in &order {
            glyf.extend_from_slice(&remap_composite(self.glyph_data(*old), &mapping));
            while glyf.len() % 4 != 0 {
                glyf.push(0);
            }
            loca.push(glyf.len() as u32);
        }

        let advance = |glyph: u16| -> (u16, i16) { self.hmetrics.get(usize::from(glyph)).copied().unwrap_or((0, 0)) };

        // Advance widths and glyph ids for the characters the document uses.
        let mut cids: BTreeMap<char, u16> = BTreeMap::new();
        let mut widths: BTreeMap<u16, u16> = BTreeMap::new();
        for (ch, old) in &chars {
            let cid = *mapping.get(old)?;
            cids.insert(*ch, cid);
            widths.insert(cid, self.scaled_width(advance(*old).0));
        }

        let mut tables: Vec<(Tag, Vec<u8>)> = Vec::new();
        tables.push((*b"head", self.subset_head()));
        tables.push((*b"hhea", self.subset_hhea(num_glyphs)));
        tables.push((*b"hmtx", self.subset_hmtx(&order, advance)));
        tables.push((*b"maxp", self.subset_maxp(num_glyphs)));
        tables.push((*b"cmap", build_cmap(&cids)?));
        tables.push((*b"glyf", glyf));
        tables.push((*b"loca", loca.iter().flat_map(|value| value.to_be_bytes()).collect()));
        tables.push((*b"post", self.subset_post()));
        for tag in [*b"name", *b"OS/2", *b"cvt ", *b"fpgm", *b"prep"] {
            if let Some(data) = self.table_bytes(tag) {
                tables.push((tag, data));
            }
        }

        let scaled = |value: i16| scale_unit(value, self.units_per_em);
        Some(Subset {
            data: assemble_sfnt(tables),
            cids,
            widths,
            bbox: [scaled(self.bbox[0]), scaled(self.bbox[1]), scaled(self.bbox[2]), scaled(self.bbox[3])],
            ascent: scaled(self.ascent),
            descent: scaled(self.descent),
            cap_height: scaled(self.cap_height),
            italic_angle: self.italic_angle,
        })
    }

    /// Advance width in the 1000-unit glyph space PDF expects.
    fn scaled_width(&self, advance: u16) -> u16 {
        let scaled = u32::from(advance) * 1000 / u32::from(self.units_per_em);
        scaled.min(u32::from(u16::MAX)) as u16
    }

    fn subset_head(&self) -> Vec<u8> {
        let mut head = self.table_bytes(*b"head").unwrap_or_default();
        head.resize(54, 0);
        head[8..12].copy_from_slice(&0u32.to_be_bytes());
        // Long `loca` offsets keep every glyph start addressable at any size.
        head[50..52].copy_from_slice(&1i16.to_be_bytes());
        head[52..54].copy_from_slice(&0i16.to_be_bytes());
        head
    }

    fn subset_hhea(&self, num_glyphs: u16) -> Vec<u8> {
        let mut hhea = self.table_bytes(*b"hhea").unwrap_or_default();
        hhea.resize(36, 0);
        hhea[34..36].copy_from_slice(&num_glyphs.to_be_bytes());
        hhea
    }

    fn subset_hmtx(&self, order: &[u16], advance: impl Fn(u16) -> (u16, i16)) -> Vec<u8> {
        let mut hmtx = Vec::with_capacity(order.len() * 4);
        for old in order {
            let (width, bearing) = advance(*old);
            hmtx.extend_from_slice(&width.to_be_bytes());
            hmtx.extend_from_slice(&bearing.to_be_bytes());
        }
        hmtx
    }

    fn subset_maxp(&self, num_glyphs: u16) -> Vec<u8> {
        let mut maxp = self.table_bytes(*b"maxp").unwrap_or_default();
        if maxp.len() < 6 {
            maxp.resize(6, 0);
        }
        maxp[4..6].copy_from_slice(&num_glyphs.to_be_bytes());
        maxp
    }

    fn subset_post(&self) -> Vec<u8> {
        let mut post = self.table_bytes(*b"post").unwrap_or_default();
        post.resize(32, 0);
        // Format 3 keeps metrics only: no glyph names, no added size.
        post[0..4].copy_from_slice(&0x0003_0000u32.to_be_bytes());
        post
    }

    fn table_bytes(&self, tag: Tag) -> Option<Vec<u8>> {
        let (offset, length) = self.tables.get(&tag)?;
        Some(self.raw[*offset..*offset + *length].to_vec())
    }
}

/// A font program subset to one document, with the CID mapping text needs.
#[derive(Debug)]
pub(crate) struct Subset {
    /// Complete subset OpenType file written into a `/FontFile2` stream.
    pub(crate) data: Vec<u8>,
    /// Character to CID (`Identity-H` treats the CID as the subset glyph id).
    pub(crate) cids: BTreeMap<char, u16>,
    /// Advance width per CID, in the 1000-unit glyph space.
    pub(crate) widths: BTreeMap<u16, u16>,
    /// `/FontBBox`, already scaled.
    pub(crate) bbox: [i16; 4],
    /// Ascender in the 1000-unit glyph space.
    pub(crate) ascent: i16,
    /// Descender in the 1000-unit glyph space.
    pub(crate) descent: i16,
    /// Cap height in the 1000-unit glyph space.
    pub(crate) cap_height: i16,
    /// Italic angle in degrees.
    pub(crate) italic_angle: f64,
}

impl Subset {
    /// CID for one character, or `None` when the subset has no glyph for it.
    pub(crate) fn cid(&self, ch: char) -> Option<u16> {
        self.cids.get(&ch).copied()
    }
}

/// Offset of the face this module reads inside `raw`, handling `.ttc` headers.
fn face_offset(raw: &[u8]) -> Option<usize> {
    let tag = raw.get(..4)?;
    if tag == b"ttcf" {
        let count = u32_at(raw, 8)?;
        if count == 0 {
            return None;
        }
        Some(u32_at(raw, 12)? as usize)
    }
    else if tag == b"OTTO" {
        // CFF outlines need `/FontFile3`, which this writer does not emit.
        None
    }
    else {
        Some(0)
    }
}

/// Table directory of one face, keyed by tag with `(offset, length)` values.
fn read_table_directory(raw: &[u8], base: usize) -> Option<BTreeMap<Tag, (usize, usize)>> {
    let count = u16_at(raw, base + 4)?;
    let mut tables = BTreeMap::new();
    for index in 0..usize::from(count) {
        let entry = base + 12 + index * 16;
        let tag: Tag = raw.get(entry..entry + 4)?.try_into().ok()?;
        let offset = u32_at(raw, entry + 8)? as usize;
        let length = u32_at(raw, entry + 12)? as usize;
        if offset.checked_add(length)? > raw.len() {
            return None;
        }
        tables.insert(tag, (offset, length));
    }
    Some(tables)
}

/// Glyph offsets, normalized to a `num_glyphs + 1` vector.
fn read_loca(raw: &[u8], loca: (usize, usize), num_glyphs: u16, index_to_loc: i16) -> Option<Vec<u32>> {
    let count = usize::from(num_glyphs) + 1;
    let mut offsets = Vec::with_capacity(count);
    for index in 0..count {
        let offset = match index_to_loc {
            0 => u32::from(u16_at(raw, loca.0 + index * 2)?) * 2,
            _ => u32_at(raw, loca.0 + index * 4)?,
        };
        offsets.push(offset);
    }
    Some(offsets)
}

/// Advance width and side bearing per glyph, expanded past `numberOfHMetrics`.
fn read_hmetrics(raw: &[u8], hmtx: (usize, usize), horizontal_count: u16, num_glyphs: u16) -> Option<Vec<(u16, i16)>> {
    let long_count = usize::from(horizontal_count.max(1)).min(usize::from(num_glyphs));
    let mut metrics = Vec::with_capacity(usize::from(num_glyphs));
    let mut last_width = 0u16;
    for index in 0..long_count {
        let at = hmtx.0 + index * 4;
        if at + 4 > raw.len() {
            return None;
        }
        last_width = u16_at(raw, at)?;
        metrics.push((last_width, i16_at(raw, at + 2)?));
    }
    let tail = hmtx.0 + long_count * 4;
    for index in long_count..usize::from(num_glyphs) {
        let at = tail + (index - long_count) * 2;
        metrics.push((last_width, i16_at(raw, at).unwrap_or(0)));
    }
    Some(metrics)
}

/// Character map of a font, normalized so lookup takes a Unicode scalar value.
#[derive(Debug)]
enum Cmap {
    /// Format 4: BMP-only segments.
    Format4 { data: Vec<u8>, segments: Vec<Segment> },
    /// Format 12: grouped ranges, including code points above the BMP.
    Format12(Vec<(u32, u32, u32)>),
}

/// One format 4 segment, with the byte position of its `idRangeOffset` word.
#[derive(Debug)]
struct Segment {
    start: u16,
    end: u16,
    delta: i16,
    range_offset_at: usize,
}

impl Cmap {
    fn parse(raw: &[u8], offset: usize, length: usize) -> Option<Self> {
        let table = raw.get(offset..offset + length)?;
        let count = u16_at(table, 2)?;
        let mut best4: Option<usize> = None;
        let mut best12: Option<usize> = None;
        for index in 0..usize::from(count) {
            let record = 4 + index * 8;
            let platform = u16_at(table, record)?;
            let encoding = u16_at(table, record + 2)?;
            let at = u32_at(table, record + 4)? as usize;
            let format = u16_at(table, at)?;
            // Prefer the full-Unicode subtable, then the Windows BMP one.
            if format == 12 {
                best12 = Some(at);
            }
            else if format == 4 && (platform == 3 && encoding == 1 || platform == 0) {
                best4 = Some(at);
            }
        }
        if let Some(at) = best12 {
            let data = table.get(at..)?.to_vec();
            return Some(Self::Format12(read_format12(&data)?));
        }
        let data = table.get(best4?..)?.to_vec();
        let segments = read_format4(&data)?;
        Some(Self::Format4 { data, segments })
    }

    fn lookup(&self, code: u32) -> Option<u16> {
        match self {
            Self::Format12(groups) => {
                let mut low = 0usize;
                let mut high = groups.len();
                while low < high {
                    let mid = (low + high) / 2;
                    let (start, end, first) = groups[mid];
                    if code < start {
                        high = mid;
                    }
                    else if code > end {
                        low = mid + 1;
                    }
                    else {
                        return u16::try_from(u32::from(first) + (code - start)).ok();
                    }
                }
                None
            }
            Self::Format4 { data, segments } => {
                let code = u16::try_from(code).ok()?;
                for segment in segments {
                    if code < segment.start || code > segment.end {
                        continue;
                    }
                    let range_offset = i16_at(data, segment.range_offset_at)?;
                    if range_offset == 0 {
                        return Some((i32::from(code) + i32::from(segment.delta)) as u16);
                    }
                    let index_at = (segment.range_offset_at as i64
                        + i64::from(range_offset)
                        + 2 * i64::from(code - segment.start)) as usize;
                    let glyph = u16_at(data, index_at)?;
                    if glyph == 0 {
                        return None;
                    }
                    return Some((i32::from(glyph) + i32::from(segment.delta)) as u16);
                }
                None
            }
        }
    }
}

/// Groups of a format 12 subtable as `(first code point, last, first glyph)`.
fn read_format12(data: &[u8]) -> Option<Vec<(u32, u32, u32)>> {
    let count = u32_at(data, 12)? as usize;
    let mut groups = Vec::with_capacity(count);
    for index in 0..count {
        let at = 16 + index * 12;
        groups.push((u32_at(data, at)?, u32_at(data, at + 4)?, u32_at(data, at + 8)?));
    }
    Some(groups)
}

/// Segments of a format 4 subtable, with their `idRangeOffset` positions.
fn read_format4(data: &[u8]) -> Option<Vec<Segment>> {
    let count = u16_at(data, 6)? as usize / 2;
    let end_at = 14;
    let start_at = end_at + count * 2 + 2;
    let delta_at = start_at + count * 2;
    let range_at = delta_at + count * 2;
    let mut segments = Vec::with_capacity(count);
    for index in 0..count {
        segments.push(Segment {
            start: u16_at(data, start_at + index * 2)?,
            end: u16_at(data, end_at + index * 2)?,
            delta: i16_at(data, delta_at + index * 2)?,
            range_offset_at: range_at + index * 2,
        });
    }
    Some(segments)
}

/// Byte offsets of the glyph id words inside a composite glyph.
///
/// Simple glyphs return an empty list, so the same helper decides whether a
/// glyph needs component rewrites.
fn component_slots(glyph: &[u8]) -> Vec<usize> {
    let mut slots = Vec::new();
    if glyph.len() < 10 || i16_at(glyph, 0).unwrap_or(0) >= 0 {
        return slots;
    }
    let mut at = 10;
    loop {
        let Some(flags) = u16_at(glyph, at)
        else {
            break;
        };
        let Some(_) = u16_at(glyph, at + 2)
        else {
            break;
        };
        slots.push(at + 2);
        let mut next = at + 4;
        next += if flags & 0x0001 != 0 { 4 } else { 2 };
        if flags & 0x0008 != 0 {
            next += 2;
        }
        if flags & 0x0040 != 0 {
            next += 4;
        }
        if flags & 0x0080 != 0 {
            next += 8;
        }
        at = next;
        if flags & 0x0020 == 0 {
            break;
        }
    }
    slots
}

/// Referenced glyph ids of a composite glyph, in component order.
fn composite_components(glyph: &[u8]) -> Vec<u16> {
    component_slots(glyph).into_iter().filter_map(|slot| u16_at(glyph, slot)).collect()
}

/// Copy a glyph, rewriting component ids into their subset ids.
fn remap_composite(glyph: &[u8], mapping: &BTreeMap<u16, u16>) -> Vec<u8> {
    let slots = component_slots(glyph);
    if slots.is_empty() {
        return glyph.to_vec();
    }
    let mut out = glyph.to_vec();
    for slot in slots {
        if let Some(new) = u16_at(glyph, slot).and_then(|old| mapping.get(&old).copied()) {
            out[slot..slot + 2].copy_from_slice(&new.to_be_bytes());
        }
    }
    out
}

/// Build a `cmap` table that maps the used characters to their subset glyphs.
fn build_cmap(cids: &BTreeMap<char, u16>) -> Option<Vec<u8>> {
    let mut entries: Vec<(u32, u16)> = cids.iter().map(|(ch, cid)| (*ch as u32, *cid)).collect();
    entries.sort_by_key(|(code, _)| *code);
    let bmp: Vec<(u16, u16)> =
        entries.iter().filter(|(code, _)| *code <= 0xFFFF).map(|(code, cid)| (*code as u16, *cid)).collect();
    let mut subtables: Vec<(u16, u16, Vec<u8>)> = vec![(3, 1, build_format4(&bmp))];
    if entries.iter().any(|(code, _)| *code > 0xFFFF) {
        subtables.push((3, 10, build_format12(&entries)));
    }

    let mut header = Vec::new();
    header.extend_from_slice(&0u16.to_be_bytes());
    header.extend_from_slice(&(subtables.len() as u16).to_be_bytes());
    let mut body = Vec::new();
    let mut offset = 4 + subtables.len() * 8;
    for (platform, encoding, subtable) in &subtables {
        header.extend_from_slice(&platform.to_be_bytes());
        header.extend_from_slice(&encoding.to_be_bytes());
        header.extend_from_slice(&(offset as u32).to_be_bytes());
        body.extend_from_slice(subtable);
        offset += subtable.len();
    }
    header.extend(body);
    Some(header)
}

/// Format 4 subtable with one segment per character plus the required terminator.
fn build_format4(entries: &[(u16, u16)]) -> Vec<u8> {
    let mut segments: Vec<(u16, u16, i16)> =
        entries.iter().map(|(code, cid)| (*code, *code, (i32::from(*cid) - i32::from(*code)) as i16)).collect();
    segments.push((0xFFFF, 0xFFFF, 1));
    let count = segments.len();

    let mut power = 1usize;
    let mut log = 0usize;
    while power * 2 <= count {
        power *= 2;
        log += 1;
    }

    let mut out = Vec::with_capacity(16 + count * 8);
    out.extend_from_slice(&4u16.to_be_bytes());
    out.extend_from_slice(&((16 + count * 8) as u16).to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&((count * 2) as u16).to_be_bytes());
    out.extend_from_slice(&((power * 2) as u16).to_be_bytes());
    out.extend_from_slice(&(log as u16).to_be_bytes());
    out.extend_from_slice(&(((count - power) * 2) as u16).to_be_bytes());
    for (_, end, _) in &segments {
        out.extend_from_slice(&end.to_be_bytes());
    }
    out.extend_from_slice(&0u16.to_be_bytes());
    for (start, _, _) in &segments {
        out.extend_from_slice(&start.to_be_bytes());
    }
    for (_, _, delta) in &segments {
        out.extend_from_slice(&delta.to_be_bytes());
    }
    for _ in 0..count {
        out.extend_from_slice(&0u16.to_be_bytes());
    }
    out
}

/// Format 12 subtable with one group per character.
fn build_format12(entries: &[(u32, u16)]) -> Vec<u8> {
    let mut out = Vec::with_capacity(16 + entries.len() * 12);
    out.extend_from_slice(&12u16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&((16 + entries.len() * 12) as u32).to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(&(entries.len() as u32).to_be_bytes());
    for (code, cid) in entries {
        out.extend_from_slice(&code.to_be_bytes());
        out.extend_from_slice(&code.to_be_bytes());
        out.extend_from_slice(&u32::from(*cid).to_be_bytes());
    }
    out
}

/// Lay out a set of tables into a complete sfnt file and fix the head checksum.
fn assemble_sfnt(mut tables: Vec<(Tag, Vec<u8>)>) -> Vec<u8> {
    tables.sort_by_key(|(tag, _)| *tag);
    let count = tables.len() as u16;
    let mut power = 1u16;
    let mut log = 0u16;
    while power * 2 <= count {
        power *= 2;
        log += 1;
    }

    let mut directory = Vec::with_capacity(12 + tables.len() * 16);
    directory.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    directory.extend_from_slice(&count.to_be_bytes());
    directory.extend_from_slice(&(power * 16).to_be_bytes());
    directory.extend_from_slice(&log.to_be_bytes());
    directory.extend_from_slice(&(count * 16 - power * 16).to_be_bytes());

    let mut offset = 12 + tables.len() * 16;
    let mut body = Vec::new();
    let mut head_offset = None;
    for (tag, data) in &tables {
        directory.extend_from_slice(tag);
        directory.extend_from_slice(&table_checksum(data).to_be_bytes());
        directory.extend_from_slice(&(offset as u32).to_be_bytes());
        directory.extend_from_slice(&(data.len() as u32).to_be_bytes());
        if tag == b"head" {
            head_offset = Some(offset);
        }
        body.extend_from_slice(data);
        let padding = (4 - data.len() % 4) % 4;
        body.resize(body.len() + padding, 0);
        offset += data.len() + padding;
    }

    let mut file = directory;
    file.extend(body);
    if let Some(head) = head_offset {
        let adjustment = 0xB1B0_AFBAu32.wrapping_sub(table_checksum(&file));
        file[head + 8..head + 12].copy_from_slice(&adjustment.to_be_bytes());
    }
    file
}

/// Big-endian u32 checksum of a table, zero padded to a multiple of four.
fn table_checksum(data: &[u8]) -> u32 {
    let mut sum = 0u32;
    for chunk in data.chunks(4) {
        let mut word = [0u8; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        sum = sum.wrapping_add(u32::from_be_bytes(word));
    }
    sum
}

/// Font units to the 1000-unit glyph space PDF uses.
fn scale_unit(value: i16, units_per_em: u16) -> i16 {
    (i32::from(value) * 1000 / i32::from(units_per_em)) as i16
}

/// Cap height from `OS/2`, falling back to the ascender.
fn read_cap_height(raw: &[u8], tables: &BTreeMap<Tag, (usize, usize)>, ascent: i16) -> i16 {
    if let Some((offset, length)) = tables.get(b"OS/2") {
        if *length >= 96 && u16_at(raw, *offset).unwrap_or(0) >= 2 {
            if let Some(value) = i16_at(raw, *offset + 88) {
                if value > 0 {
                    return value;
                }
            }
        }
    }
    ascent
}

/// Italic angle in degrees from the `post` table.
fn read_italic_angle(raw: &[u8], tables: &BTreeMap<Tag, (usize, usize)>) -> f64 {
    if let Some((offset, _)) = tables.get(b"post") {
        if let Some(value) = u32_at(raw, *offset + 4) {
            return f64::from(value as i32) / 65536.0;
        }
    }
    0.0
}

/// Read a big-endian u16 at `at`, or `None` when the range is out of bounds.
fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(data.get(at..at.checked_add(2)?)?.try_into().ok()?))
}

/// Read a big-endian i16 at `at`.
fn i16_at(data: &[u8], at: usize) -> Option<i16> {
    Some(i16::from_be_bytes(data.get(at..at.checked_add(2)?)?.try_into().ok()?))
}

/// Read a big-endian u32 at `at`.
fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(data.get(at..at.checked_add(4)?)?.try_into().ok()?))
}
