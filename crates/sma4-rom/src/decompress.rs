//! Reimplementations of the GBA BIOS decompression routines (SWI 0x11-0x15).
//!
//! Every stream starts with a 4-byte header: bits 4-7 of byte 0 give the type,
//! bytes 1-3 the decompressed size.

pub type Result<T> = std::result::Result<T, &'static str>;

pub const TYPE_LZ77: u8 = 0x10;
pub const TYPE_HUFFMAN: u8 = 0x20;
pub const TYPE_RLE: u8 = 0x30;

/// Upper bound to reject garbage headers when scanning; nothing in a 8 MiB game is larger.
const MAX_SIZE: usize = 0x40000;

fn header(src: &[u8]) -> Result<(u8, usize)> {
    if src.len() < 4 {
        return Err("truncated header");
    }
    let size = u32::from_le_bytes([src[1], src[2], src[3], 0]) as usize;
    if size == 0 || size > MAX_SIZE {
        return Err("implausible size");
    }
    Ok((src[0], size))
}

pub fn decompress(src: &[u8]) -> Result<Vec<u8>> {
    match src.first().map(|b| b & 0xF0) {
        Some(TYPE_LZ77) => lz77(src),
        Some(TYPE_HUFFMAN) => huffman(src),
        Some(TYPE_RLE) => rle(src),
        _ => Err("unknown compression type"),
    }
}

/// LZ77UnCompWram / LZ77UnCompVram (SWI 0x11/0x12). Output is identical for both.
pub fn lz77(src: &[u8]) -> Result<Vec<u8>> {
    lz77_with_len(src).map(|(out, _)| out)
}

/// Like [`lz77`], also returning how many source bytes were consumed.
pub fn lz77_with_len(src: &[u8]) -> Result<(Vec<u8>, usize)> {
    let (kind, size) = header(src)?;
    if kind != TYPE_LZ77 {
        return Err("not LZ77");
    }
    let mut out = Vec::with_capacity(size);
    let mut i = 4;
    let byte = |i: usize| src.get(i).copied().ok_or("truncated LZ77 stream");
    while out.len() < size {
        let flags = byte(i)?;
        i += 1;
        for bit in (0..8).rev() {
            if out.len() >= size {
                break;
            }
            if flags & (1 << bit) == 0 {
                out.push(byte(i)?);
                i += 1;
            } else {
                let b0 = byte(i)? as usize;
                let b1 = byte(i + 1)? as usize;
                i += 2;
                let len = (b0 >> 4) + 3;
                let disp = (((b0 & 0xF) << 8) | b1) + 1;
                if disp > out.len() {
                    return Err("LZ77 back-reference before start");
                }
                for _ in 0..len.min(size - out.len()) {
                    out.push(out[out.len() - disp]);
                }
            }
        }
    }
    Ok((out, i))
}

/// RLUnCompWram / RLUnCompVram (SWI 0x14/0x15).
pub fn rle(src: &[u8]) -> Result<Vec<u8>> {
    let (kind, size) = header(src)?;
    if kind != TYPE_RLE {
        return Err("not RLE");
    }
    let mut out = Vec::with_capacity(size);
    let mut i = 4;
    let byte = |i: usize| src.get(i).copied().ok_or("truncated RLE stream");
    while out.len() < size {
        let flag = byte(i)?;
        i += 1;
        if flag & 0x80 != 0 {
            let len = (flag & 0x7F) as usize + 3;
            let v = byte(i)?;
            i += 1;
            out.extend(std::iter::repeat_n(v, len.min(size - out.len())));
        } else {
            let len = (flag as usize + 1).min(size - out.len());
            out.extend_from_slice(src.get(i..i + len).ok_or("truncated RLE stream")?);
            i += len;
        }
    }
    Ok(out)
}

/// HuffUnComp (SWI 0x13), 4- or 8-bit symbols.
pub fn huffman(src: &[u8]) -> Result<Vec<u8>> {
    let (kind, size) = header(src)?;
    let bits = (kind & 0x0F) as u32;
    if kind & 0xF0 != TYPE_HUFFMAN || !(bits == 4 || bits == 8) {
        return Err("not Huffman");
    }
    let tree_size = (*src.get(4).ok_or("truncated Huffman stream")? as usize + 1) * 2;
    let tree = 5; // root node offset
    let mut pos = 4 + tree_size;
    let mut out = Vec::with_capacity(size);
    let mut acc: u32 = 0;
    let mut acc_bits = 0;
    let mut node = tree;

    while out.len() < size {
        let word = src
            .get(pos..pos + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .ok_or("truncated Huffman stream")?;
        pos += 4;
        for bit in (0..32).rev() {
            let n = *src.get(node).ok_or("bad Huffman tree")?;
            let right = (word >> bit) & 1 == 1;
            // Children of node at `node` sit at (node & !1) + offset*2 + 2.
            let child = (node & !1) + ((n & 0x3F) as usize) * 2 + 2 + right as usize;
            let is_leaf = n & if right { 0x40 } else { 0x80 } != 0;
            if is_leaf {
                acc |= (*src.get(child).ok_or("bad Huffman tree")? as u32) << acc_bits;
                acc_bits += bits;
                node = tree;
                if acc_bits == 32 {
                    out.extend_from_slice(&acc.to_le_bytes());
                    acc = 0;
                    acc_bits = 0;
                    if out.len() >= size {
                        break;
                    }
                }
            } else {
                node = child;
            }
        }
    }
    out.truncate(size);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lz77_literals_and_backref() {
        // "ABCABCABC": 3 literals then a back-reference of length 6, distance 3.
        let src = [0x10, 9, 0, 0, 0b0001_0000, b'A', b'B', b'C', 0x30, 0x02];
        assert_eq!(lz77(&src).unwrap(), b"ABCABCABC");
    }

    #[test]
    fn rle_runs_and_raw() {
        // Raw "xy", then a run of 5 'z'.
        let src = [0x30, 7, 0, 0, 0x01, b'x', b'y', 0x82, b'z'];
        assert_eq!(rle(&src).unwrap(), b"xyzzzzz");
    }

    #[test]
    fn huffman_8bit_two_symbols() {
        // Tree: root with two leaves 'a' (bit 0) and 'b' (bit 1).
        // tree_size byte = 1 -> (1+1)*2 = 4 bytes: [size, root, 'a', 'b'].
        let mut src = vec![0x28, 4, 0, 0, 1, 0b1100_0000, b'a', b'b'];
        // Bits (MSB first): a b b a -> 0110, rest zero.
        src.extend_from_slice(&0x6000_0000u32.to_le_bytes());
        assert_eq!(huffman(&src).unwrap(), b"abba");
    }

    #[test]
    fn rejects_bad_backref() {
        let src = [0x10, 4, 0, 0, 0b1000_0000, 0x00, 0x05];
        assert!(lz77(&src).is_err());
    }
}
