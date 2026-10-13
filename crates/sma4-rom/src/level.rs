//! Level data: the extracted metatile map plus metatile definitions and solidity read
//! straight from the ROM. Addresses are for the European ROM (AX4P rev 0).
//!
//! See docs/re-notes.md ("Level storage") for how these structures were found.

use crate::gfx::{Bgr555, MapEntry};
use crate::{Rom, Result};

/// Per-tileset pointers to metatile definitions: 4 tilemap entries per metatile.
const METATILE_TABLES: u32 = 0x0813_5C0C;
/// Definitions at or above this entry index come from the shared table.
const TILESET_ENTRIES: u32 = 0x498;
const SHARED_METATILES: u32 = 0x0814_EA74;
/// Per-tileset pointers to 8 solidity thresholds (4 "solid on top", 4 "solid").
const SOLIDITY_TABLES: u32 = 0x0813_1A38;

pub const METATILE: usize = 16;

/// One metatile as stored in the page buffers: an id plus the high-layer flag (+0x100).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Metatile {
    pub id: u8,
    pub high: bool,
}

/// Renders one metatile to 16x16 RGBA from 4bpp BG character data (as left in VRAM by
/// the level loaders) and BG palette RAM. Colour 0 of each palette is transparent.
pub fn render_metatile(chars: &[u8], palette: &[Bgr555], entries: [MapEntry; 4]) -> Vec<u8> {
    let mut out = vec![0u8; METATILE * METATILE * 4];
    // Entry order matches the column drawer: top-left, bottom-left, top-right, bottom-right.
    for (k, e) in entries.iter().enumerate() {
        let (ox, oy) = ((k / 2) * 8, (k % 2) * 8);
        let start = (e.tile as usize * 32).min(chars.len() - 32);
        let tile = &chars[start..start + 32];
        for py in 0..8 {
            for px in 0..8 {
                let sx = if e.hflip { 7 - px } else { px };
                let sy = if e.vflip { 7 - py } else { py };
                let b = tile[sy * 4 + sx / 2];
                let ci = if sx & 1 == 1 { b >> 4 } else { b & 0xF };
                if ci == 0 {
                    continue;
                }
                let c = palette[e.palette as usize * 16 + ci as usize];
                let o = ((oy + py) * METATILE + ox + px) * 4;
                out[o..o + 4].copy_from_slice(&c.to_rgba(255));
            }
        }
    }
    out
}

impl Rom {
    /// The four tilemap entries (TL, BL, TR, BR) that make up a metatile.
    pub fn metatile_entries(&self, tileset: u8, m: Metatile) -> Result<[MapEntry; 4]> {
        let table = self.ptr(METATILE_TABLES + tileset as u32 * 4)?;
        let base = m.id as u32 * 4 + if m.high { 0x400 } else { 0 };
        let mut out = [MapEntry::from(0); 4];
        for (k, slot) in out.iter_mut().enumerate() {
            let i = base + k as u32;
            let src = if i < TILESET_ENTRIES { table } else { SHARED_METATILES };
            *slot = MapEntry::from(self.u16(src + i * 2)?);
        }
        Ok(out)
    }

    pub fn solidity(&self, tileset: u8) -> Result<Solidity> {
        let t = self.ptr(SOLIDITY_TABLES + tileset as u32 * 4)?;
        let b = self.slice(t, 8)?;
        Ok(Solidity { top: [b[0], b[1], b[2], b[3]], solid: [b[4], b[5], b[6], b[7]] })
    }
}

/// How the player collides with a metatile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TileKind {
    #[default]
    Empty,
    /// Can be stood on, passed through from below and the sides.
    Top,
    Solid,
}

/// SMB3-style thresholds: within each quadrant (id >> 6), ids at or above `solid` are
/// solid and ids at or above `top` can be stood on.
#[derive(Debug, Clone, Copy)]
pub struct Solidity {
    pub top: [u8; 4],
    pub solid: [u8; 4],
}

impl Solidity {
    pub fn kind(&self, m: Metatile) -> TileKind {
        if m.high {
            // High-layer metatiles are decorations/specials; collision TBD.
            return TileKind::Empty;
        }
        let q = (m.id >> 6) as usize;
        if m.id >= self.solid[q] {
            TileKind::Solid
        } else if m.id >= self.top[q] {
            TileKind::Top
        } else {
            TileKind::Empty
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solidity_thresholds_per_quadrant() {
        let s = Solidity { top: [0x25, 0x50, 0xA0, 0xE2], solid: [0x2D, 0x53, 0xAD, 0xF0] };
        let m = |id| Metatile { id, high: false };
        assert_eq!(s.kind(m(0x80)), TileKind::Empty);
        assert_eq!(s.kind(m(0x53)), TileKind::Solid);
        assert_eq!(s.kind(m(0x50)), TileKind::Top);
        assert_eq!(s.kind(m(0x00)), TileKind::Empty);
    }
}

/// Enemy-list pointers, indexed by enemy-list id (from the world map or a level header).
const ENEMY_LISTS: u32 = 0x0813_6B14;

/// One enemy-list record: object type, metatile column and row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnemyRecord {
    pub kind: u8,
    pub col: u8,
    pub row: u8,
}

impl Rom {
    /// Reads an enemy list: one header byte, then 4-byte records `[00, type, col, row]`
    /// until 0xFF. (A few lists in the ROM contain longer records; not decoded yet.)
    pub fn enemy_list(&self, id: u16) -> Result<Vec<EnemyRecord>> {
        let mut p = self.ptr(ENEMY_LISTS + id as u32 * 4)? + 1;
        let mut out = Vec::new();
        while self.u8(p)? != 0xFF {
            let r = self.slice(p, 4)?;
            out.push(EnemyRecord { kind: r[1], col: r[2], row: r[3] });
            p += 4;
        }
        Ok(out)
    }
}
