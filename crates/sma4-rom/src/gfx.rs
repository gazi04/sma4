//! GBA graphics formats: BGR555 palettes and 4bpp/8bpp 8x8 tiles.

pub const TILE: usize = 8;
pub const TILE_BYTES_4BPP: usize = 32;
pub const TILE_BYTES_8BPP: usize = 64;

/// BGR555 colour as stored in palette RAM.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Bgr555(pub u16);

impl Bgr555 {
    /// Expands to 8-bit channels, replicating the high bits so 31 maps to 255.
    pub fn to_rgba(self, alpha: u8) -> [u8; 4] {
        let c = |s: u16| {
            let v = ((self.0 >> s) & 0x1F) as u8;
            (v << 3) | (v >> 2)
        };
        [c(0), c(5), c(10), alpha]
    }
}

pub fn parse_palette(bytes: &[u8]) -> Vec<Bgr555> {
    bytes
        .chunks_exact(2)
        .map(|b| Bgr555(u16::from_le_bytes([b[0], b[1]])))
        .collect()
}

/// Splits 4bpp tile data into per-pixel palette indices (64 per tile).
pub fn tiles_4bpp(bytes: &[u8]) -> Vec<[u8; 64]> {
    bytes
        .chunks_exact(TILE_BYTES_4BPP)
        .map(|t| {
            let mut px = [0u8; 64];
            for (i, b) in t.iter().enumerate() {
                px[i * 2] = b & 0x0F;
                px[i * 2 + 1] = b >> 4;
            }
            px
        })
        .collect()
}

pub fn tiles_8bpp(bytes: &[u8]) -> Vec<[u8; 64]> {
    bytes
        .chunks_exact(TILE_BYTES_8BPP)
        .map(|t| t.try_into().unwrap())
        .collect()
}

/// A tilemap entry for text-mode backgrounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapEntry {
    pub tile: u16,
    pub hflip: bool,
    pub vflip: bool,
    pub palette: u8,
}

impl From<u16> for MapEntry {
    fn from(v: u16) -> Self {
        Self {
            tile: v & 0x3FF,
            hflip: v & 0x400 != 0,
            vflip: v & 0x800 != 0,
            palette: (v >> 12) as u8,
        }
    }
}

/// Renders tiles into an RGBA sheet `cols` tiles wide. Index 0 is transparent,
/// matching how the GBA treats colour 0 of every palette.
pub fn tile_sheet_rgba(tiles: &[[u8; 64]], palette: &[Bgr555], cols: usize) -> (Vec<u8>, usize, usize) {
    let rows = tiles.len().div_ceil(cols).max(1);
    let (w, h) = (cols * TILE, rows * TILE);
    let mut out = vec![0u8; w * h * 4];
    for (n, tile) in tiles.iter().enumerate() {
        let (tx, ty) = ((n % cols) * TILE, (n / cols) * TILE);
        for (p, &idx) in tile.iter().enumerate() {
            if idx == 0 {
                continue;
            }
            let c = palette.get(idx as usize).copied().unwrap_or_default();
            let (x, y) = (tx + p % TILE, ty + p / TILE);
            out[(y * w + x) * 4..][..4].copy_from_slice(&c.to_rgba(255));
        }
    }
    (out, w, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bgr555_extremes() {
        assert_eq!(Bgr555(0x7FFF).to_rgba(255), [255, 255, 255, 255]);
        assert_eq!(Bgr555(0x001F).to_rgba(255), [255, 0, 0, 255]);
        assert_eq!(Bgr555(0x7C00).to_rgba(255), [0, 0, 255, 255]);
    }

    #[test]
    fn nibble_order_low_first() {
        let mut t = [0u8; 32];
        t[0] = 0x21;
        assert_eq!(&tiles_4bpp(&t)[0][..2], &[1, 2]);
    }
}
