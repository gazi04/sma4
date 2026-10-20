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

    /// A blank AX4P image big enough for the enemy-list tables.
    fn blank_rom() -> Vec<u8> {
        let mut d = vec![0u8; 0x17_0000];
        d[0xAC..0xB0].copy_from_slice(b"AX4P");
        d[0xBD] = d[0xA0..=0xBC].iter().fold(0u8, |c, &b| c.wrapping_sub(b)).wrapping_sub(0x19);
        d
    }

    #[test]
    fn enemy_list_records_take_their_types_extra_bytes() {
        let mut d = blank_rom();
        let list = 0x10_0000u32;
        d[(ENEMY_LISTS - 0x0800_0000) as usize..][..4].copy_from_slice(&(0x0800_0000 + list).to_le_bytes());
        // 0xE0 takes two extra bytes (flags 0x0C), 0x195 none, 0x72 none.
        d[(LIST_FLAGS_LOW - 0x0800_0000) as usize + 0xE0] = 0x0C;
        let bytes = [0x00, 0x00, 0xE0, 0x18, 0x18, 0x00, 0x80, 0x01, 0x95, 0x19, 0x13, 0x00, 0x72, 0x20, 0x18, 0xFF];
        d[list as usize..][..bytes.len()].copy_from_slice(&bytes);
        let rom = crate::Rom::from_bytes(d).unwrap();
        let l = rom.enemy_list(0).unwrap();
        assert_eq!(
            l,
            [
                EnemyRecord { kind: 0xE0, col: 0x18, row: 0x18, extra: [Some(0), Some(0x80)] },
                EnemyRecord { kind: 0x195, col: 0x19, row: 0x13, extra: [None, None] },
                EnemyRecord { kind: 0x72, col: 0x20, row: 0x18, extra: [None, None] },
            ]
        );
    }
}

/// Enemy-list pointers, indexed by enemy-list id (from the world map or a level header).
const ENEMY_LISTS: u32 = 0x0813_6B14;

/// Per-type flags of object types below 0x160 (bit 2: one extra record byte, bit 3: two)
/// and of types 0x160..0x18F (bit 0: one, bit 1: two), indexed by the full type.
const LIST_FLAGS_LOW: u32 = 0x0816_A9CA;
const LIST_FLAGS_HIGH: u32 = 0x0816_ABF1;

/// One enemy-list record: object type, metatile column and row, and the extra bytes
/// some types take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnemyRecord {
    pub kind: u16,
    pub col: u8,
    pub row: u8,
    pub extra: [Option<u8>; 2],
}

impl Rom {
    /// Reads an enemy list as 0x08004F38 copies it: one header byte, then records
    /// `[type hi, type lo, col, row]` until 0xFF, each followed by the 0-2 extra bytes its
    /// type's flags ask for (0x1AB always takes one).
    pub fn enemy_list(&self, id: u16) -> Result<Vec<EnemyRecord>> {
        self.enemy_list_at(self.ptr(ENEMY_LISTS + id as u32 * 4)?)
    }

    /// An enemy list by address (some areas name theirs directly, e.g. 0x03003B10).
    pub fn enemy_list_at(&self, addr: u32) -> Result<Vec<EnemyRecord>> {
        let mut p = addr + 1;
        let mut out = Vec::new();
        while self.u8(p)? != 0xFF {
            let r = self.slice(p, 4)?;
            let kind = u16::from_be_bytes([r[0], r[1]]);
            p += 4;
            let (one, two) = match kind {
                0..0x160 => {
                    let f = self.u8(LIST_FLAGS_LOW + kind as u32)?;
                    (f & 4 != 0, f & 8 != 0)
                }
                0x160..0x190 => {
                    let f = self.u8(LIST_FLAGS_HIGH + kind as u32)?;
                    (f & 1 != 0, f & 2 != 0)
                }
                0x1AB => (true, false),
                _ => (false, false),
            };
            let mut extra = [None; 2];
            if one {
                extra[0] = Some(self.u8(p)?);
                p += 1;
                if two {
                    extra[1] = Some(self.u8(p)?);
                    p += 1;
                }
            }
            out.push(EnemyRecord { kind, col: r[2], row: r[3], extra });
        }
        Ok(out)
    }
}
