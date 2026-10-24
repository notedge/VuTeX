//! Scene image resources: reference resolution and PDF image dictionaries.
//!
//! A Davinci scene image node carries a `ResourceRef` rather than pixels, so
//! this module turns the reference into an embeddable raster. `data:` URIs with
//! a base64 payload and filesystem paths are accepted; any other scheme is
//! refused and the caller keeps its dashed placeholder.
//!
//! PNG and JPEG are decoded only as far as legal PDF embedding needs. A JPEG
//! keeps its compressed stream and is wrapped in `/DCTDecode`, with the frame
//! header supplying the size and colour space. A PNG is inflated to 8-bit
//! samples and written without a filter. The crate takes no dependencies, so
//! the module carries its own base64, zlib/DEFLATE (stored, fixed-Huffman, and
//! dynamic-Huffman blocks), PNG filter, and JPEG marker parsers.
//!
//! Every parser is bounds-checked and fails closed: an unsupported format,
//! corrupt header, or zero-sized image returns `None` instead of panicking or
//! emitting an invalid dictionary.

use std::{fs, path::Path};

/// PNG file signature, the first eight bytes of every PNG stream.
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// JPEG start-of-image marker.
const JPEG_START: [u8; 2] = [0xFF, 0xD8];

/// Largest inflated sample buffer this module accepts, in bytes.
///
/// The cap keeps a corrupt or hostile header from asking for an enormous
/// allocation before the DEFLATE stream is validated.
const MAX_SAMPLES: usize = 64 * 1024 * 1024;

/// Number of bits in a DEFLATE Huffman code.
const MAX_BITS: usize = 15;

/// A raster image ready to become a PDF image XObject.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Raster {
    /// Complete image stream dictionary, including `/Length`.
    pub(crate) dict: String,
    /// Stream body: a JPEG stream or unfiltered 8-bit samples.
    pub(crate) data: Vec<u8>,
}

/// Resolve, sniff, and decode the image behind one scene `href`.
///
/// Returns `None` when the reference cannot be read, is not a PNG or JPEG, or
/// carries a header this module cannot parse.
pub(crate) fn load(href: &str) -> Option<Raster> {
    let raw = read_source(href)?;
    if raw.starts_with(&PNG_SIGNATURE) {
        decode_png(&raw)
    }
    else if raw.starts_with(&JPEG_START) {
        decode_jpeg(&raw)
    }
    else {
        None
    }
}

/// Bytes behind one `href`: an inline `data:` payload or a file's contents.
fn read_source(href: &str) -> Option<Vec<u8>> {
    let href = href.trim();
    if href.is_empty() {
        return None;
    }
    if href.get(..5).is_some_and(|prefix| prefix.eq_ignore_ascii_case("data:")) {
        return base64_payload(&href[5..]);
    }
    if has_remote_scheme(href) {
        return None;
    }
    fs::read(Path::new(href)).ok()
}

/// Whether a reference names a remote or non-file URI scheme.
///
/// A single ASCII letter is treated as a Windows drive prefix (`C:\…`) rather
/// than a scheme, so absolute paths keep working on Windows.
fn has_remote_scheme(href: &str) -> bool {
    let Some((scheme, _)) = href.split_once(':')
    else {
        return false;
    };
    let is_drive = scheme.len() == 1 && scheme.chars().all(|ch| ch.is_ascii_alphabetic());
    !is_drive && !scheme.is_empty() && scheme.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.'))
}

/// Decode the payload of a `data:` URI, which must carry a base64 body.
fn base64_payload(rest: &str) -> Option<Vec<u8>> {
    let (meta, payload) = rest.split_once(',')?;
    if !meta.split(';').skip(1).any(|parameter| parameter.trim().eq_ignore_ascii_case("base64")) {
        return None;
    }
    base64_decode(payload)
}

/// Decode standard base64, ignoring whitespace and stopping at the padding.
fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let mut buffer = 0u32;
    let mut bits = 0u32;
    for byte in text.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            b' ' | b'\t' | b'\r' | b'\n' => continue,
            _ => return None,
        };
        buffer = (buffer << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
            buffer &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// PNG header fields this module needs.
#[derive(Debug, Clone, Copy)]
struct PngHeader {
    /// Pixel width in samples.
    width: usize,
    /// Pixel height in rows.
    height: usize,
    /// PNG colour type, which selects the channel layout.
    color_type: u8,
}

impl PngHeader {
    /// Parse and validate the thirteen-byte `IHDR` body.
    ///
    /// Only 8-bit, non-interlaced truecolour, greyscale, grey-alpha, and
    /// RGBA inputs are accepted; 16-bit and packed low-bit-depth samples would
    /// need a different embedding path.
    fn parse(body: &[u8]) -> Option<Self> {
        if body.len() < 13 {
            return None;
        }
        let width = u32_at(body, 0)? as usize;
        let height = u32_at(body, 4)? as usize;
        let bit_depth = *body.get(8)?;
        let color_type = *body.get(9)?;
        let compression = *body.get(10)?;
        let filter = *body.get(11)?;
        let interlace = *body.get(12)?;
        if width == 0 || height == 0 || bit_depth != 8 || compression != 0 || filter != 0 || interlace != 0 {
            return None;
        }
        if !matches!(color_type, 0 | 2 | 3 | 4 | 6) {
            return None;
        }
        Some(Self { width, height, color_type })
    }

    /// Samples per pixel in the inflated scanlines.
    fn channels(self) -> usize {
        match self.color_type {
            0 | 3 => 1,
            4 => 2,
            6 => 4,
            _ => 3,
        }
    }
}

/// Decode a PNG into unfiltered 8-bit samples.
fn decode_png(raw: &[u8]) -> Option<Raster> {
    let mut header: Option<PngHeader> = None;
    let mut palette: Option<Vec<u8>> = None;
    let mut compressed = Vec::new();
    let mut cursor = PNG_SIGNATURE.len();
    loop {
        let length = u32_at(raw, cursor)? as usize;
        let kind = raw.get(cursor + 4..cursor + 8)?;
        let body_start = cursor.checked_add(8)?;
        let body_end = body_start.checked_add(length)?;
        let body = raw.get(body_start..body_end)?;
        match kind {
            b"IHDR" => header = Some(PngHeader::parse(body)?),
            b"PLTE" => palette = Some(body.to_vec()),
            b"IDAT" => compressed.extend_from_slice(body),
            b"IEND" => break,
            _ => {}
        }
        // Skip the four-byte CRC; this decoder neither needs nor verifies it.
        cursor = body_end.checked_add(4)?;
    }
    let header = header?;
    let channels = header.channels();
    let row_bytes = header.width.checked_mul(channels)?;
    let expected = header.height.checked_mul(row_bytes.checked_add(1)?)?;
    if expected > MAX_SAMPLES {
        return None;
    }
    let inflated = inflate(&compressed, expected)?;
    let scanlines = inflated.get(..expected)?;
    let samples = unfilter(scanlines, header.height, row_bytes, channels)?;
    let (data, color_space) = match header.color_type {
        0 => (samples, "DeviceGray"),
        4 => (drop_alpha(&samples, channels), "DeviceGray"),
        6 => (drop_alpha(&samples, channels), "DeviceRGB"),
        3 => (expand_palette(&samples, palette.as_deref()?), "DeviceRGB"),
        _ => (samples, "DeviceRGB"),
    };
    Some(raster(header.width, header.height, color_space, None, data))
}

/// Reverse PNG scanline filters, returning rows without filter bytes.
fn unfilter(scanlines: &[u8], height: usize, row_bytes: usize, bpp: usize) -> Option<Vec<u8>> {
    let mut rows = Vec::with_capacity(height * row_bytes);
    let mut previous = vec![0u8; row_bytes];
    for row in 0..height {
        let start = row * (row_bytes + 1);
        let filter = *scanlines.get(start)?;
        let source = scanlines.get(start + 1..start + 1 + row_bytes)?;
        let mut current = vec![0u8; row_bytes];
        for index in 0..row_bytes {
            let left = if index >= bpp { current[index - bpp] } else { 0 };
            let up = previous[index];
            let up_left = if index >= bpp { previous[index - bpp] } else { 0 };
            current[index] = match filter {
                0 => source[index],
                1 => source[index].wrapping_add(left),
                2 => source[index].wrapping_add(up),
                3 => source[index].wrapping_add(((u16::from(left) + u16::from(up)) / 2) as u8),
                4 => source[index].wrapping_add(paeth(left, up, up_left)),
                _ => return None,
            };
        }
        rows.extend_from_slice(&current);
        previous = current;
    }
    Some(rows)
}

/// PNG Paeth predictor: the neighbour closest to `left + up - up_left`.
fn paeth(left: u8, up: u8, up_left: u8) -> u8 {
    let estimate = i16::from(left) + i16::from(up) - i16::from(up_left);
    let distance_left = (estimate - i16::from(left)).abs();
    let distance_up = (estimate - i16::from(up)).abs();
    let distance_up_left = (estimate - i16::from(up_left)).abs();
    if distance_left <= distance_up && distance_left <= distance_up_left {
        left
    }
    else if distance_up <= distance_up_left {
        up
    }
    else {
        up_left
    }
}

/// Drop the alpha byte of every pixel in a grey-alpha or RGBA buffer.
fn drop_alpha(samples: &[u8], channels: usize) -> Vec<u8> {
    samples.chunks_exact(channels).flat_map(|pixel| pixel.iter().copied().take(channels - 1)).collect()
}

/// Expand a palette index buffer into RGB triplets.
///
/// An index past the palette becomes black rather than failing the whole
/// image, so a slightly corrupt palette still paints.
fn expand_palette(samples: &[u8], palette: &[u8]) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(samples.len() * 3);
    for &index in samples {
        let at = usize::from(index) * 3;
        match palette.get(at..at + 3) {
            Some(entry) => rgb.extend_from_slice(entry),
            None => rgb.extend_from_slice(&[0, 0, 0]),
        }
    }
    rgb
}

/// Image dictionary plus stream body for one decoded raster.
fn raster(width: usize, height: usize, color_space: &str, filter: Option<&str>, data: Vec<u8>) -> Raster {
    let filter = filter.map(|filter| format!(" /Filter /{filter}")).unwrap_or_default();
    let dict = format!(
        "<< /Type /XObject /Subtype /Image /Width {width} /Height {height} \
         /ColorSpace /{color_space} /BitsPerComponent 8{filter} /Length {} >>",
        data.len()
    );
    Raster { dict, data }
}

/// Parse a JPEG frame header and keep the compressed stream unchanged.
///
/// `DCTDecode` accepts the JPEG stream as authored, so only the dimensions and
/// component count are read: three components mean RGB, one greyscale, and
/// four CMYK.
fn decode_jpeg(raw: &[u8]) -> Option<Raster> {
    if raw.get(..2)? != JPEG_START {
        return None;
    }
    let mut at = 2;
    while at + 1 < raw.len() {
        if raw[at] != 0xFF {
            return None;
        }
        // Segments may be preceded by any number of 0xFF fill bytes.
        let mut marker_at = at;
        while raw.get(marker_at + 1) == Some(&0xFF) {
            marker_at += 1;
        }
        let marker = *raw.get(marker_at + 1)?;
        at = marker_at + 2;
        match marker {
            // Standalone markers carry no length field.
            0x01 | 0xD0..=0xD7 => continue,
            // End of image, or the scan itself, before any frame header.
            0xD9 | 0xDA => return None,
            0xD8 => continue,
            _ => {
                let length = usize::from(u16_at(raw, at)?);
                if length < 2 {
                    return None;
                }
                let end = at.checked_add(length)?;
                let body = raw.get(at + 2..end)?;
                if is_frame_header(marker) {
                    return jpeg_frame(raw, body);
                }
                at = end;
            }
        }
    }
    None
}

/// Whether a marker starts a JPEG frame header (every `SOFn` except `DHT`).
fn is_frame_header(marker: u8) -> bool {
    matches!(marker, 0xC0 | 0xC1 | 0xC2 | 0xC3 | 0xC5 | 0xC6 | 0xC7 | 0xC9 | 0xCA | 0xCB | 0xCD | 0xCE | 0xCF)
}

/// Build the raster for a JPEG whose frame header body was located.
fn jpeg_frame(raw: &[u8], body: &[u8]) -> Option<Raster> {
    if body.len() < 6 || *body.first()? != 8 {
        return None;
    }
    let height = usize::from(u16_at(body, 1)?);
    let width = usize::from(u16_at(body, 3)?);
    let components = usize::from(*body.get(5)?);
    let color_space = match components {
        1 => "DeviceGray",
        3 => "DeviceRGB",
        4 => "DeviceCMYK",
        _ => return None,
    };
    if width == 0 || height == 0 {
        return None;
    }
    Some(raster(width, height, color_space, Some("DCTDecode"), raw.to_vec()))
}

/// Inflate a zlib stream, refusing more output than `limit` bytes.
///
/// The Adler-32 trailer is not verified: the stream has already been bounded
/// by the caller's expected sample size.
fn inflate(zlib: &[u8], limit: usize) -> Option<Vec<u8>> {
    let cmf = *zlib.first()?;
    let flg = *zlib.get(1)?;
    if cmf & 0x0F != 8 {
        return None;
    }
    if cmf >> 4 > 7 {
        return None;
    }
    if (u16::from(cmf) * 256 + u16::from(flg)) % 31 != 0 {
        return None;
    }
    // A preset dictionary would need bytes this module does not carry.
    if flg & 0x20 != 0 {
        return None;
    }
    let mut bits = Bits { data: zlib, bit: 2 * 8 };
    let mut out = Vec::with_capacity(limit);
    loop {
        let last = bits.bits(1)?;
        match bits.bits(2)? {
            0 => inflate_stored(&mut bits, &mut out, limit)?,
            1 => {
                let literals = fixed_literals()?;
                let distances = fixed_distances()?;
                inflate_codes(&mut bits, &mut out, &literals, &distances, limit)?;
            }
            2 => {
                let (literals, distances) = dynamic_trees(&mut bits)?;
                inflate_codes(&mut bits, &mut out, &literals, &distances, limit)?;
            }
            _ => return None,
        }
        if last == 1 {
            return Some(out);
        }
    }
}

/// Little-endian bit reader, the order DEFLATE packs its codes in.
#[derive(Debug)]
struct Bits<'a> {
    /// Whole compressed stream.
    data: &'a [u8],
    /// Absolute bit offset of the next bit to read.
    bit: usize,
}

impl Bits<'_> {
    /// Read `count` bits, least significant bit first.
    fn bits(&mut self, count: u8) -> Option<u32> {
        let mut value = 0u32;
        for index in 0..u32::from(count) {
            let byte = *self.data.get(self.bit / 8)?;
            value |= u32::from((byte >> (self.bit % 8)) & 1) << index;
            self.bit += 1;
        }
        Some(value)
    }

    /// Move to the next byte boundary.
    fn align(&mut self) {
        self.bit = (self.bit + 7) / 8 * 8;
    }

    /// Copy `count` whole bytes from the current byte position.
    fn take(&mut self, count: usize) -> Option<&[u8]> {
        let start = self.bit / 8;
        let end = start.checked_add(count)?;
        let bytes = self.data.get(start..end)?;
        self.bit = end.checked_mul(8)?;
        Some(bytes)
    }
}

/// Copy one stored (uncompressed) DEFLATE block.
fn inflate_stored(bits: &mut Bits<'_>, out: &mut Vec<u8>, limit: usize) -> Option<()> {
    bits.align();
    let length = u16::from_le_bytes(bits.take(2)?.try_into().ok()?);
    let complement = u16::from_le_bytes(bits.take(2)?.try_into().ok()?);
    if length != !complement {
        return None;
    }
    let length = usize::from(length);
    if out.len().checked_add(length)? > limit {
        return None;
    }
    out.extend_from_slice(bits.take(length)?);
    Some(())
}

/// Expand literal/length and distance codes until the end-of-block symbol.
fn inflate_codes(bits: &mut Bits<'_>, out: &mut Vec<u8>, literals: &Huffman, distances: &Huffman, limit: usize) -> Option<()> {
    loop {
        let symbol = usize::from(literals.decode(bits)?);
        match symbol {
            0..=255 => {
                if out.len() >= limit {
                    return None;
                }
                out.push(symbol as u8);
            }
            256 => return Some(()),
            257..=285 => {
                let index = symbol - 257;
                let length = usize::from(LENGTH_BASE[index]) + bits.bits(LENGTH_EXTRA[index])? as usize;
                let distance_symbol = usize::from(distances.decode(bits)?);
                let base = *DISTANCE_BASE.get(distance_symbol)?;
                let distance = usize::from(base) + bits.bits(DISTANCE_EXTRA[distance_symbol])? as usize;
                if distance == 0 || distance > out.len() {
                    return None;
                }
                if out.len().checked_add(length)? > limit {
                    return None;
                }
                for _ in 0..length {
                    let byte = out[out.len() - distance];
                    out.push(byte);
                }
            }
            _ => return None,
        }
    }
}

/// Read the code length code, then the literal and distance trees, of a
/// dynamic-Huffman block.
fn dynamic_trees(bits: &mut Bits<'_>) -> Option<(Huffman, Huffman)> {
    let literal_count = bits.bits(5)? as usize + 257;
    let distance_count = bits.bits(5)? as usize + 1;
    let code_count = bits.bits(4)? as usize + 4;
    if literal_count > 286 || distance_count > 30 {
        return None;
    }
    const ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
    let mut code_lengths = [0u8; 19];
    for &slot in ORDER.iter().take(code_count) {
        code_lengths[slot] = bits.bits(3)? as u8;
    }
    let code_tree = Huffman::new(&code_lengths)?;
    let total = literal_count + distance_count;
    let mut lengths = Vec::with_capacity(total);
    while lengths.len() < total {
        match code_tree.decode(bits)? {
            symbol @ 0..=15 => lengths.push(symbol as u8),
            16 => {
                let last = *lengths.last()?;
                let repeat = 3 + bits.bits(2)? as usize;
                lengths.extend(std::iter::repeat_n(last, repeat));
            }
            17 => {
                let repeat = 3 + bits.bits(3)? as usize;
                lengths.extend(std::iter::repeat_n(0, repeat));
            }
            18 => {
                let repeat = 11 + bits.bits(7)? as usize;
                lengths.extend(std::iter::repeat_n(0, repeat));
            }
            _ => return None,
        }
        if lengths.len() > total {
            return None;
        }
    }
    let distance_lengths = lengths.split_off(literal_count);
    Some((Huffman::new(&lengths)?, Huffman::new(&distance_lengths)?))
}

/// Canonical Huffman tree used by the DEFLATE decoder.
#[derive(Debug)]
struct Huffman {
    /// Number of codes of each bit length; index zero counts unused symbols.
    counts: [u16; MAX_BITS + 1],
    /// Symbols ordered by code length, then by symbol value.
    symbols: Vec<u16>,
}

impl Huffman {
    /// Build a tree from per-symbol code lengths; over-subscribed codes fail.
    fn new(lengths: &[u8]) -> Option<Self> {
        let mut counts = [0u16; MAX_BITS + 1];
        for &length in lengths {
            let length = usize::from(length);
            if length > MAX_BITS {
                return None;
            }
            counts[length] += 1;
        }
        let mut remaining = 1i32;
        for length in 1..=MAX_BITS {
            remaining = (remaining << 1) - i32::from(counts[length]);
            if remaining < 0 {
                return None;
            }
        }
        let mut offsets = [0u16; MAX_BITS + 2];
        for length in 1..=MAX_BITS {
            offsets[length + 1] = offsets[length] + counts[length];
        }
        let mut symbols = vec![0u16; lengths.len().saturating_sub(usize::from(counts[0]))];
        for (symbol, &length) in lengths.iter().enumerate() {
            if length == 0 {
                continue;
            }
            let slot = offsets[usize::from(length)] as usize;
            let target = symbols.get_mut(slot)?;
            *target = symbol as u16;
            offsets[usize::from(length)] += 1;
        }
        Some(Self { counts, symbols })
    }

    /// Decode one symbol, following the canonical code bit by bit.
    fn decode(&self, bits: &mut Bits<'_>) -> Option<u16> {
        let mut code = 0i32;
        let mut first = 0i32;
        let mut index = 0i32;
        for length in 1..=MAX_BITS {
            code |= bits.bits(1)? as i32;
            let count = i32::from(self.counts[length]);
            if code - count < first {
                return usize::try_from(index + (code - first)).ok().and_then(|slot| self.symbols.get(slot).copied());
            }
            index += count;
            first = (first + count) << 1;
            code <<= 1;
        }
        None
    }
}

/// Literal/length tree defined by the DEFLATE fixed-Huffman block.
fn fixed_literals() -> Option<Huffman> {
    let mut lengths = [0u8; 288];
    for (symbol, length) in lengths.iter_mut().enumerate() {
        *length = match symbol {
            0..=143 => 8,
            144..=255 => 9,
            256..=279 => 7,
            _ => 8,
        };
    }
    Huffman::new(&lengths)
}

/// Distance tree defined by the DEFLATE fixed-Huffman block.
fn fixed_distances() -> Option<Huffman> {
    Huffman::new(&[5u8; 32])
}

/// Length bases of the DEFLATE length codes 257 through 285.
const LENGTH_BASE: [u16; 29] =
    [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];

/// Extra bits read after each DEFLATE length code.
const LENGTH_EXTRA: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];

/// Distance bases of the DEFLATE distance codes.
const DISTANCE_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193,
    12289, 16385, 24577,
];

/// Extra bits read after each DEFLATE distance code.
const DISTANCE_EXTRA: [u8; 30] =
    [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];

/// Read a big-endian `u16` at `at`, or `None` when the range is out of bounds.
fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(data.get(at..at.checked_add(2)?)?.try_into().ok()?))
}

/// Read a big-endian `u32` at `at`.
fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(data.get(at..at.checked_add(4)?)?.try_into().ok()?))
}
