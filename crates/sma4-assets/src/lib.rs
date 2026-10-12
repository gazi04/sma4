//! On-disk formats for assets extracted from the user's ROM by `sma4-extract`.
//!
//! Layout of the asset directory (default `assets/extracted/`, git-ignored, never shared):
//! ```text
//! manifest.txt              which ROM the assets came from
//! levels/level_XXX.bin      level layout (metatile ids), solidity, backdrop (XXX = level id, hex)
//! levels/level_XXX.png      all 512 metatiles of the level's tileset, 16x16 each, stacked
//! gfx/XXXXXXXX.png          every LZ77 tile blob in the ROM (greyscale), for the viewer
//! ```
//! The game reads only these files; it never opens the ROM.

use std::path::{Path, PathBuf};

pub const METATILE: u32 = 16;
pub const DEFAULT_DIR: &str = "assets/extracted";

const LEVEL_MAGIC: &[u8; 8] = b"SMA4LV03";

#[derive(Debug, thiserror::Error)]
pub enum AssetError {
    #[error("{path}: {source}")]
    Io { path: PathBuf, source: std::io::Error },
    #[error("{0}: not a level asset (re-run sma4-extract)")]
    BadLevel(PathBuf),
    #[error("{path}: bad PNG: {msg}")]
    Png { path: PathBuf, msg: String },
}

pub type Result<T> = std::result::Result<T, AssetError>;

fn read(path: &Path) -> Result<Vec<u8>> {
    std::fs::read(path).map_err(|source| AssetError::Io { path: path.into(), source })
}

fn write(path: &Path, data: &[u8]) -> Result<()> {
    let io = |source| AssetError::Io { path: path.into(), source };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(io)?;
    }
    std::fs::write(path, data).map_err(io)
}

/// How the player collides with a metatile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum TileKind {
    #[default]
    Empty = 0,
    /// Can be stood on; passable from below and the sides.
    Top = 1,
    Solid = 2,
}

impl TileKind {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Top,
            2 => Self::Solid,
            _ => Self::Empty,
        }
    }
}

/// 8-bit RGBA image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Rgba {
    pub fn write_png(&self, path: &Path) -> Result<()> {
        let png_err = |e: png::EncodingError| AssetError::Png { path: path.into(), msg: e.to_string() };
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, self.width, self.height);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            let mut w = enc.write_header().map_err(png_err)?;
            w.write_image_data(&self.pixels).map_err(png_err)?;
        }
        write(path, &out)
    }

    pub fn read_png(path: &Path) -> Result<Self> {
        let png_err = |msg: String| AssetError::Png { path: path.into(), msg };
        let data = read(path)?;
        let mut dec = png::Decoder::new(std::io::Cursor::new(data));
        dec.set_transformations(png::Transformations::EXPAND | png::Transformations::ALPHA);
        let mut reader = dec.read_info().map_err(|e| png_err(e.to_string()))?;
        let mut buf = vec![0; reader.output_buffer_size().ok_or_else(|| png_err("image too large".into()))?];
        let info = reader.next_frame(&mut buf).map_err(|e| png_err(e.to_string()))?;
        if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
            return Err(png_err(format!("expected 8-bit RGBA, got {:?}", info.color_type)));
        }
        buf.truncate(info.buffer_size());
        Ok(Self { width: info.width, height: info.height, pixels: buf })
    }
}

/// SMA4/SMB3 solidity rule: within each quadrant (low byte >> 6), ids at or above
/// `solid[q]` are solid and ids at or above `top[q]` can be stood on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Solidity {
    pub top: [u8; 4],
    pub solid: [u8; 4],
}

impl Solidity {
    pub fn kind(&self, metatile: u16) -> TileKind {
        if metatile >= 0x100 {
            // High-layer metatiles are decorations/specials; collision TBD.
            return TileKind::Empty;
        }
        let id = metatile as u8;
        let q = (id >> 6) as usize;
        if id >= self.solid[q] {
            TileKind::Solid
        } else if id >= self.top[q] {
            TileKind::Top
        } else {
            TileKind::Empty
        }
    }
}

/// Metatiles per tileset: 256 ids, plus 256 more on the high layer.
pub const METATILES: u32 = 512;

/// A level ready to draw and play.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LevelAsset {
    pub level_id: u16,
    pub tileset: u8,
    /// Size in metatiles.
    pub width: usize,
    pub height: usize,
    pub backdrop: [u8; 3],
    pub solidity: Solidity,
    /// Metatile per cell, row-major from the top-left: id | 0x100 for the high layer.
    pub metatiles: Vec<u16>,
    /// All `METATILES` metatiles of the tileset rendered at 16x16, stacked vertically:
    /// metatile `m` is rows `16*m .. 16*m+16`.
    pub atlas: Rgba,
}

impl LevelAsset {
    pub fn paths(dir: &Path, level_id: u16) -> (PathBuf, PathBuf) {
        let stem = dir.join("levels").join(format!("level_{level_id:03x}"));
        (stem.with_extension("bin"), stem.with_extension("png"))
    }

    pub fn metatile(&self, x: usize, y: usize) -> u16 {
        self.metatiles[y * self.width + x]
    }

    pub fn kind(&self, x: usize, y: usize) -> TileKind {
        self.solidity.kind(self.metatile(x, y))
    }

    pub fn write(&self, dir: &Path) -> Result<()> {
        let (bin, png) = Self::paths(dir, self.level_id);
        let mut out = Vec::with_capacity(32 + self.metatiles.len() * 2);
        out.extend_from_slice(LEVEL_MAGIC);
        out.extend_from_slice(&self.level_id.to_le_bytes());
        out.extend_from_slice(&(self.width as u16).to_le_bytes());
        out.extend_from_slice(&(self.height as u16).to_le_bytes());
        out.extend_from_slice(&self.backdrop);
        out.push(self.tileset);
        out.extend_from_slice(&self.solidity.top);
        out.extend_from_slice(&self.solidity.solid);
        for m in &self.metatiles {
            out.extend_from_slice(&m.to_le_bytes());
        }
        write(&bin, &out)?;
        self.atlas.write_png(&png)
    }

    pub fn read(dir: &Path, level_id: u16) -> Result<Self> {
        let (bin, png) = Self::paths(dir, level_id);
        let d = read(&bin)?;
        let bad = || AssetError::BadLevel(bin.clone());
        const HEADER: usize = 26;
        if d.len() < HEADER || &d[..8] != LEVEL_MAGIC {
            return Err(bad());
        }
        let u16_at = |o: usize| u16::from_le_bytes([d[o], d[o + 1]]);
        let (width, height) = (u16_at(10) as usize, u16_at(12) as usize);
        let body = &d[HEADER..];
        if body.len() != width * height * 2 {
            return Err(bad());
        }
        let metatiles = body.chunks_exact(2).map(|b| u16::from_le_bytes([b[0], b[1]])).collect();
        let atlas = Rgba::read_png(&png)?;
        if atlas.width != METATILE || atlas.height != METATILE * METATILES {
            return Err(AssetError::Png { path: png, msg: "atlas must be 16 x 8192".into() });
        }
        Ok(Self {
            level_id: u16_at(8),
            tileset: d[17],
            width,
            height,
            backdrop: [d[14], d[15], d[16]],
            solidity: Solidity {
                top: [d[18], d[19], d[20], d[21]],
                solid: [d[22], d[23], d[24], d[25]],
            },
            metatiles,
            atlas,
        })
    }
}

/// Graphics sheets for the viewer, sorted by ROM address.
pub fn gfx_sheets(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir.join("gfx"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "png"))
        .collect();
    v.sort();
    v
}

pub fn gfx_path(dir: &Path, rom_addr: u32) -> PathBuf {
    dir.join("gfx").join(format!("{rom_addr:08x}.png"))
}

pub fn write_manifest(dir: &Path, text: &str) -> Result<()> {
    write(&dir.join("manifest.txt"), text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_round_trip() {
        let dir = std::env::temp_dir().join(format!("sma4-assets-test-{}", std::process::id()));
        let lvl = LevelAsset {
            level_id: 0x22,
            tileset: 1,
            width: 2,
            height: 2,
            backdrop: [1, 2, 3],
            solidity: Solidity { top: [0x25, 0x50, 0xA0, 0xE2], solid: [0x2D, 0x53, 0xAD, 0xF0] },
            metatiles: vec![0x80, 0x53, 0x150, 0x00],
            atlas: Rgba {
                width: METATILE,
                height: METATILE * METATILES,
                pixels: (0..(METATILE * METATILE * METATILES * 4)).map(|i| i as u8).collect(),
            },
        };
        lvl.write(&dir).unwrap();
        assert_eq!(LevelAsset::read(&dir, 0x22).unwrap(), lvl);
        assert_eq!(lvl.kind(1, 0), TileKind::Solid);
        assert_eq!(lvl.kind(0, 1), TileKind::Empty);
        std::fs::remove_dir_all(dir).ok();
    }
}
