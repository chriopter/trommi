//! The snapshot file's wrapping (spec/v1.md 10.8): gzip (RFC 1952) around DEFLATE (RFC 1951), written and read here
//! so that the core needs no compression crate.
//!
//! Writing uses the fixed Huffman codes and a plain LZ77 search (one hash table of three-byte prefixes, a short chain
//! of earlier places): JSON of shapes shrinks well with it, and any reader takes it. Reading takes every DEFLATE block
//! kind (stored, fixed, dynamic), as a browser's `CompressionStream` writes them, and never gives more than `max`
//! bytes: whoever decompresses a file enforces its limit.

/// Why the bytes are no gzip file this reader takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GzipError {
    /// Not gzip, or a damaged stream, or a checksum that does not hold.
    Bad,
    /// More than the limit once decompressed.
    TooLarge,
}

// ---- CRC-32 (the gzip trailer's) ----

fn crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    for (n, slot) in table.iter_mut().enumerate() {
        let mut c = n as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *slot = c;
    }
    table
}

fn crc32(bytes: &[u8]) -> u32 {
    let table = crc_table();
    let mut c = !0u32;
    for b in bytes {
        c = table[((c ^ u32::from(*b)) & 0xff) as usize] ^ (c >> 8);
    }
    !c
}

// ---- the tables of DEFLATE (RFC 1951, 3.2.5) ----

const LENGTH_BASE: [u16; 29] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LENGTH_EXTRA: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];
/// The order in which a dynamic block lists the code lengths of its code-length code.
const CLEN_ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];

// ---- writing ----

struct BitsOut {
    out: Vec<u8>,
    acc: u64,
    n: u32,
}

impl BitsOut {
    /// `count` bits of `value`, lowest first.
    fn put(&mut self, value: u32, count: u32) {
        self.acc |= u64::from(value) << self.n;
        self.n += count;
        while self.n >= 8 {
            self.out.push(self.acc as u8);
            self.acc >>= 8;
            self.n -= 8;
        }
    }
    /// A Huffman code: its bits highest first.
    fn code(&mut self, code: u32, len: u32) {
        let mut reversed = 0;
        for i in 0..len {
            reversed |= ((code >> i) & 1) << (len - 1 - i);
        }
        self.put(reversed, len);
    }
    fn finish(mut self) -> Vec<u8> {
        if self.n > 0 {
            self.out.push(self.acc as u8);
        }
        self.out
    }
}

/// A literal or end-of-block (256) or length symbol (257..=285) in the fixed code.
fn fixed_symbol(bits: &mut BitsOut, symbol: u16) {
    let s = u32::from(symbol);
    match s {
        0..=143 => bits.code(0x30 + s, 8),
        144..=255 => bits.code(0x190 + s - 144, 9),
        256..=279 => bits.code(s - 256, 7),
        _ => bits.code(0xc0 + s - 280, 8),
    }
}

fn put_length(bits: &mut BitsOut, length: usize) {
    let i = LENGTH_BASE.iter().rposition(|b| usize::from(*b) <= length).unwrap_or(0);
    fixed_symbol(bits, 257 + i as u16);
    bits.put((length - usize::from(LENGTH_BASE[i])) as u32, u32::from(LENGTH_EXTRA[i]));
}

fn put_distance(bits: &mut BitsOut, distance: usize) {
    let i = DIST_BASE.iter().rposition(|b| usize::from(*b) <= distance).unwrap_or(0);
    bits.code(i as u32, 5);
    bits.put((distance - usize::from(DIST_BASE[i])) as u32, u32::from(DIST_EXTRA[i]));
}

const WINDOW: usize = 32 * 1024;
const MAX_MATCH: usize = 258;
const HASH_BITS: u32 = 15;
/// How many earlier places of one prefix are tried.
const CHAIN: usize = 32;

fn hash3(b: &[u8]) -> usize {
    let v = u32::from(b[0]) << 16 | u32::from(b[1]) << 8 | u32::from(b[2]);
    (v.wrapping_mul(2_654_435_761) >> (32 - HASH_BITS)) as usize
}

/// `bytes` as one gzip member.
pub fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut bits = BitsOut { out: vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 0xff], acc: 0, n: 0 };
    // one block, final, fixed codes
    bits.put(1, 1);
    bits.put(1, 2);
    let none = usize::MAX;
    let mut head = vec![none; 1 << HASH_BITS];
    let mut prev = vec![none; bytes.len()];
    let mut at = 0;
    while at < bytes.len() {
        let mut best = (0usize, 0usize);
        if at + 3 <= bytes.len() {
            let h = hash3(&bytes[at..]);
            let mut candidate = head[h];
            let mut tries = 0;
            while candidate != none && at - candidate <= WINDOW && tries < CHAIN {
                let limit = MAX_MATCH.min(bytes.len() - at);
                let mut len = 0;
                while len < limit && bytes[candidate + len] == bytes[at + len] {
                    len += 1;
                }
                if len > best.0 {
                    best = (len, at - candidate);
                    if len == limit {
                        break;
                    }
                }
                candidate = prev[candidate];
                tries += 1;
            }
        }
        let step = if best.0 >= 3 {
            put_length(&mut bits, best.0);
            put_distance(&mut bits, best.1);
            best.0
        } else {
            fixed_symbol(&mut bits, u16::from(bytes[at]));
            1
        };
        for p in at..at + step {
            if p + 3 <= bytes.len() {
                let h = hash3(&bytes[p..]);
                prev[p] = head[h];
                head[h] = p;
            }
        }
        at += step;
    }
    fixed_symbol(&mut bits, 256);
    let mut out = bits.finish();
    out.extend_from_slice(&crc32(bytes).to_le_bytes());
    out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    out
}

// ---- reading ----

struct BitsIn<'a> {
    bytes: &'a [u8],
    at: usize,
    acc: u64,
    n: u32,
}

impl BitsIn<'_> {
    fn bits(&mut self, count: u32) -> Result<u32, GzipError> {
        while self.n < count {
            let b = *self.bytes.get(self.at).ok_or(GzipError::Bad)?;
            self.at += 1;
            self.acc |= u64::from(b) << self.n;
            self.n += 8;
        }
        let v = (self.acc & ((1u64 << count) - 1)) as u32;
        self.acc >>= count;
        self.n -= count;
        Ok(v)
    }
    fn align(&mut self) {
        self.acc = 0;
        self.n = 0;
    }
}

/// A canonical Huffman code for decoding: how many codes of each length, and the symbols in code order.
struct Huffman {
    counts: [u16; 16],
    symbols: Vec<u16>,
}

impl Huffman {
    fn new(lengths: &[u8]) -> Result<Huffman, GzipError> {
        let mut counts = [0u16; 16];
        for l in lengths {
            counts[usize::from(*l)] += 1;
        }
        counts[0] = 0;
        // over-subscribed: no code at all
        let mut left: i32 = 1;
        for c in &counts[1..] {
            left = left * 2 - i32::from(*c);
            if left < 0 {
                return Err(GzipError::Bad);
            }
        }
        let mut offsets = [0u16; 16];
        for len in 1..15 {
            offsets[len + 1] = offsets[len] + counts[len];
        }
        let mut symbols = vec![0u16; lengths.len()];
        for (symbol, l) in lengths.iter().enumerate() {
            if *l != 0 {
                symbols[usize::from(offsets[usize::from(*l)])] = symbol as u16;
                offsets[usize::from(*l)] += 1;
            }
        }
        Ok(Huffman { counts, symbols })
    }

    fn decode(&self, input: &mut BitsIn<'_>) -> Result<u16, GzipError> {
        let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
        for len in 1..16 {
            code |= input.bits(1)? as i32;
            let count = i32::from(self.counts[len]);
            if code - count < first {
                return self.symbols.get((index + code - first) as usize).copied().ok_or(GzipError::Bad);
            }
            index += count;
            first += count;
            first <<= 1;
            code <<= 1;
        }
        Err(GzipError::Bad)
    }
}

fn inflate_codes(input: &mut BitsIn<'_>, out: &mut Vec<u8>, max: usize, lit: &Huffman, dist: &Huffman) -> Result<(), GzipError> {
    loop {
        let symbol = lit.decode(input)?;
        match symbol {
            0..=255 => {
                if out.len() >= max {
                    return Err(GzipError::TooLarge);
                }
                out.push(symbol as u8);
            }
            256 => return Ok(()),
            257..=285 => {
                let i = usize::from(symbol - 257);
                let length = usize::from(LENGTH_BASE[i]) + input.bits(u32::from(LENGTH_EXTRA[i]))? as usize;
                let d = usize::from(dist.decode(input)?);
                if d >= 30 {
                    return Err(GzipError::Bad);
                }
                let distance = usize::from(DIST_BASE[d]) + input.bits(u32::from(DIST_EXTRA[d]))? as usize;
                if distance > out.len() {
                    return Err(GzipError::Bad);
                }
                if out.len() + length > max {
                    return Err(GzipError::TooLarge);
                }
                let from = out.len() - distance;
                for k in 0..length {
                    let b = out[from + k];
                    out.push(b);
                }
            }
            _ => return Err(GzipError::Bad),
        }
    }
}

fn fixed_codes() -> Result<(Huffman, Huffman), GzipError> {
    let mut lengths = [0u8; 288];
    for (s, l) in lengths.iter_mut().enumerate() {
        *l = match s {
            0..=143 => 8,
            144..=255 => 9,
            256..=279 => 7,
            _ => 8,
        };
    }
    Ok((Huffman::new(&lengths)?, Huffman::new(&[5u8; 30])?))
}

fn dynamic_codes(input: &mut BitsIn<'_>) -> Result<(Huffman, Huffman), GzipError> {
    let nlen = input.bits(5)? as usize + 257;
    let ndist = input.bits(5)? as usize + 1;
    let ncode = input.bits(4)? as usize + 4;
    if nlen > 286 || ndist > 30 {
        return Err(GzipError::Bad);
    }
    let mut clens = [0u8; 19];
    for i in 0..ncode {
        clens[CLEN_ORDER[i]] = input.bits(3)? as u8;
    }
    let clen = Huffman::new(&clens)?;
    let mut lengths = vec![0u8; nlen + ndist];
    let mut i = 0;
    while i < nlen + ndist {
        let symbol = clen.decode(input)?;
        if symbol < 16 {
            lengths[i] = symbol as u8;
            i += 1;
            continue;
        }
        let (value, repeat) = match symbol {
            16 => (*lengths.get(i.wrapping_sub(1)).ok_or(GzipError::Bad)?, 3 + input.bits(2)? as usize),
            17 => (0, 3 + input.bits(3)? as usize),
            _ => (0, 11 + input.bits(7)? as usize),
        };
        if i + repeat > nlen + ndist {
            return Err(GzipError::Bad);
        }
        for slot in &mut lengths[i..i + repeat] {
            *slot = value;
        }
        i += repeat;
    }
    if lengths[256] == 0 {
        return Err(GzipError::Bad);
    }
    Ok((Huffman::new(&lengths[..nlen])?, Huffman::new(&lengths[nlen..])?))
}

/// The bytes of one gzip member, never more than `max` of them.
pub fn gunzip(bytes: &[u8], max: usize) -> Result<Vec<u8>, GzipError> {
    const FTEXT_HCRC: u8 = 2;
    const FEXTRA: u8 = 4;
    const FNAME: u8 = 8;
    const FCOMMENT: u8 = 16;
    if bytes.len() < 18 || bytes[0] != 0x1f || bytes[1] != 0x8b || bytes[2] != 8 || bytes[3] & 0xe0 != 0 {
        return Err(GzipError::Bad);
    }
    let flags = bytes[3];
    let mut at = 10;
    if flags & FEXTRA != 0 {
        let len = usize::from(u16::from_le_bytes([*bytes.get(at).ok_or(GzipError::Bad)?, *bytes.get(at + 1).ok_or(GzipError::Bad)?]));
        at += 2 + len;
    }
    for flag in [FNAME, FCOMMENT] {
        if flags & flag != 0 {
            let end = bytes.get(at..).and_then(|rest| rest.iter().position(|b| *b == 0)).ok_or(GzipError::Bad)?;
            at += end + 1;
        }
    }
    if flags & FTEXT_HCRC != 0 {
        at += 2;
    }
    let body = bytes.get(at..).ok_or(GzipError::Bad)?;
    let mut input = BitsIn { bytes: body, at: 0, acc: 0, n: 0 };
    let mut out = Vec::new();
    loop {
        let last = input.bits(1)? == 1;
        match input.bits(2)? {
            0 => {
                input.align();
                let header = body.get(input.at..input.at + 4).ok_or(GzipError::Bad)?;
                let len = usize::from(u16::from_le_bytes([header[0], header[1]]));
                let nlen = u16::from_le_bytes([header[2], header[3]]);
                if nlen != !(len as u16) {
                    return Err(GzipError::Bad);
                }
                let data = body.get(input.at + 4..input.at + 4 + len).ok_or(GzipError::Bad)?;
                if out.len() + len > max {
                    return Err(GzipError::TooLarge);
                }
                out.extend_from_slice(data);
                input.at += 4 + len;
            }
            1 => {
                let (lit, dist) = fixed_codes()?;
                inflate_codes(&mut input, &mut out, max, &lit, &dist)?;
            }
            2 => {
                let (lit, dist) = dynamic_codes(&mut input)?;
                inflate_codes(&mut input, &mut out, max, &lit, &dist)?;
            }
            _ => return Err(GzipError::Bad),
        }
        if last {
            break;
        }
    }
    let trailer = body.get(input.at..input.at + 8).ok_or(GzipError::Bad)?;
    let crc = u32::from_le_bytes([trailer[0], trailer[1], trailer[2], trailer[3]]);
    let size = u32::from_le_bytes([trailer[4], trailer[5], trailer[6], trailer[7]]);
    if crc != crc32(&out) || size != out.len() as u32 {
        return Err(GzipError::Bad);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Python's `gzip.compress(text, 9, mtime=0)` of the text below: fixed codes.
    const FIXED: &str = "1f8b08000000000002ffab562a53b232d6512ace482c482d56b28a8ed5514a2bcacf2bc94c2d52b2aaaed5514acfcf4b8588e7e697a5a68098b5d518ba6a01817ad5ae49000000";
    /// The same of forty shapes (`shape_text(40)`): dynamic codes.
    const DYNAMIC: &str = "1f8b08000000000002ffadd6310ac2401046e1bb4c1d486626464d1711c4d2c21318c1c5b01b74c1227877d7cede976e0379d5ff2d59248cd2cb509ebaa91ba924a7349537f33596c3254de9514e21decbe915c67c93be6b2b995388f9f9fdf274dcef86f341ded5f2d352b06560cbc1560bb65660ab035b6bb0b5015b5b72abe8f0c9e52b397d25b7afe4f8955cbf92f35772ff4a0250528091020cbdfb4901460a30528091028c1460a40023051829c049014e0a70f4f78714e0a40027053829c049014e0af07f057c00961f1bb6fe0b0000";

    fn shape_text(n: usize) -> String {
        (0..n).map(|i| format!(r#"{{"id":"AAAA/{i}/0","tool":"pen","color":"ink","width":64,"points":"AQIDBAUG"}},"#)).collect()
    }

    fn unhex(text: &str) -> Vec<u8> {
        (0..text.len() / 2).map(|i| u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).unwrap()).collect()
    }

    #[test]
    fn what_is_written_reads_back() {
        let shapes = shape_text(400);
        for text in ["", "a", "abcabcabcabcabcabc", shapes.as_str()] {
            let packed = gzip(text.as_bytes());
            assert_eq!(gunzip(&packed, 1 << 20).unwrap(), text.as_bytes());
        }
        assert!(gzip(shapes.as_bytes()).len() * 5 < shapes.len(), "it compresses");        let mut noise = Vec::new();
        let mut x = 7u32;
        for _ in 0..100_000 {
            x = x.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            noise.push((x >> 16) as u8);
        }
        assert_eq!(gunzip(&gzip(&noise), 1 << 20).unwrap(), noise);
    }

    #[test]
    fn another_writers_file_reads() {
        let got = gunzip(&unhex(FIXED), 1 << 20).unwrap();
        assert_eq!(got, br#"{"v":3,"shapes":[],"frontier":{},"gone":[],"moved":[]}{"v":3,"shapes":[]}"#);
        assert_eq!(gunzip(&unhex(DYNAMIC), 1 << 20).unwrap(), shape_text(40).as_bytes());
    }

    #[test]
    fn damage_and_size_are_refused() {
        let packed = gzip(&[b'x'; 5000]);
        assert_eq!(gunzip(&packed, 4999), Err(GzipError::TooLarge));
        let mut broken = packed.clone();
        let n = broken.len();
        broken[n - 5] ^= 1;
        assert_eq!(gunzip(&broken, 1 << 20), Err(GzipError::Bad));
        assert_eq!(gunzip(&packed[..packed.len() - 1], 1 << 20), Err(GzipError::Bad));
        assert_eq!(gunzip(b"not gzip at all, really", 1 << 20), Err(GzipError::Bad));
    }
}
