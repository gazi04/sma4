//! Block-hit tables (European ROM, AX4P rev 0). See docs/re-notes.md, "Blocks and items".
//!
//! When the player's head-centre probe (probe 1) hits a tile while moving up, the game
//! (0x0809FC1C) searches a list of id ranges for the tile, which yields a block index; the
//! index selects a behaviour (handler table 0x0817310C) and the high nibble of the result.

use crate::{Rom, Result};

const RANGE_START: u32 = 0x0817_30E8;
const RANGE_LEN: u32 = 0x0817_30CA;
const RANGE_BASE: u32 = 0x0817_30D9;
const RANGE_PROBE_MASK: u32 = 0x0817_30F7;
const PROBE_MASKS: u32 = 0x0817_3106;
const BEHAVIOUR: u32 = 0x0817_307C;
const RESULT_HI: u32 = 0x0817_30A3;
/// Tileset class (`0x03003CF1`), per tileset.
const TILESET_CLASS: u32 = 0x0817_2BCC;

/// Object type spawned per hit result (low nibble), read by the bump-block object.
const ITEM_TYPES: u32 = 0x0817_0340;
/// Final tile kind per hit result (high nibble).
const FINAL_KIND: u32 = 0x0817_036D;
/// Metatile written for a finished bump, indexed by kind - 1 (0x08027E3C).
const FINAL_METATILE: u32 = 0x0813_AA00;
/// Bump-block speed per bump timer value.
const BOUNCE: u32 = 0x0817_0377;

pub const NOT_BUMPABLE: u8 = 0xFF;
const HEAD_CENTRE_PROBE: usize = 1;
const LAND_FIRST_RANGE: usize = 3;
/// The feet probe (probe 0, frame + (0x20, 8)): landing on a note block bumps it.
const FEET_PROBE: usize = 0;

/// Tables needed to reproduce block hits, in plain form.
pub struct BlockTables {
    pub bump_behaviour: [u8; 256],
    /// The same for the feet probe while moving down or standing (note blocks).
    pub land_behaviour: [u8; 256],
    pub result_hi: [u8; 256],
    pub item_types: [u16; 16],
    pub final_kind: [u8; 16],
    pub final_metatile: [u16; 32],
    pub bounce: [i8; 11],
}

impl Rom {
    pub fn block_tables(&self, tileset: u8) -> Result<BlockTables> {
        let start = self.slice(RANGE_START, 15)?;
        let len = self.slice(RANGE_LEN, 15)?;
        let base = self.slice(RANGE_BASE, 15)?;
        let mask = self.slice(RANGE_PROBE_MASK, 15)?;
        let probe_mask = self.u8(PROBE_MASKS + HEAD_CENTRE_PROBE as u32)?;
        let feet_mask = self.u8(PROBE_MASKS + FEET_PROBE as u32)?;
        let behaviour = self.slice(BEHAVIOUR, 0x27)?;
        let result_hi = self.slice(RESULT_HI, 0x27)?;
        // Moving up, the search starts at range 6 (7 for tileset class 8) and walks down.
        let first = if self.u8(TILESET_CLASS + tileset as u32)? == 8 { 7 } else { 6 };

        let mut t = BlockTables {
            bump_behaviour: [NOT_BUMPABLE; 256],
            land_behaviour: [NOT_BUMPABLE; 256],
            result_hi: [0; 256],
            item_types: [0; 16],
            final_kind: [0; 16],
            final_metatile: [0; 32],
            bounce: [0; 11],
        };
        for tile in 0..=255u8 {
            let hit = (0..=first).rev().find(|&k| {
                tile.wrapping_sub(start[k]) < len[k] && mask[k] & probe_mask != 0
            });
            if let Some(k) = hit {
                let idx = (tile.wrapping_sub(start[k]) + base[k]) as usize;
                t.bump_behaviour[tile as usize] = behaviour[idx];
                t.result_hi[tile as usize] = result_hi[idx];
            }
            // Moving down (speed >= 0) the search starts at range 3 for every probe
            // (0x0809FC1C); the ranges it can reach are disjoint from the higher ones, so
            // `result_hi` is shared.
            let landed = (0..=LAND_FIRST_RANGE).rev().find(|&k| {
                tile.wrapping_sub(start[k]) < len[k] && mask[k] & feet_mask != 0
            });
            if let Some(k) = landed {
                let idx = (tile.wrapping_sub(start[k]) + base[k]) as usize;
                t.land_behaviour[tile as usize] = behaviour[idx];
                debug_assert!(hit == landed);
            }
        }
        for (i, v) in t.item_types.iter_mut().enumerate() {
            *v = self.u16(ITEM_TYPES + i as u32 * 2)?;
        }
        t.final_kind[..10].copy_from_slice(self.slice(FINAL_KIND, 10)?);
        for (i, v) in t.final_metatile.iter_mut().take(18).enumerate() {
            *v = self.u16(FINAL_METATILE + i as u32 * 2)?;
        }
        for (i, v) in t.bounce.iter_mut().enumerate() {
            *v = self.u8(BOUNCE + i as u32)? as i8;
        }
        Ok(t)
    }
}
