//! Coins placed in the level (metatile 0x40) and the sparkle left when one is collected.
//!
//! * Player tile probes 0x0809A7E8 -> 0x0809B508: four points of the 16x32 player frame,
//!   (dy, dx) pairs from 0x08172DB7. Each probe that lands on metatile 0x40 collects it
//!   (0x0809A450): tile change kind 1 (0x080A1D28), sound 0x5F.
//! * Kind 1 (0x08027E3C) writes 0x0813AA00[0] (the blank metatile) into the cell; the player
//!   main 0x08094780 then adds the coin and 50 points and spawns effect type 3 at the cell
//!   (0x08090AD4).
//! * Effect type 3 (0x08090C10, draw 0x08090B0C): counter +0x12 ticks on odd frames and the
//!   effect ends when it reaches 11. Four 8x8 sprites at fixed offsets, each playing tiles
//!   0x6C..0x68 from table 0x08170CE2, staggered by two ticks.

use crate::level::{Level, TILE_PX};
use crate::player::{Form, Player};

/// Coin metatile (the `'@'` compare in 0x0809A450).
pub const COIN: u16 = 0x40;

/// Probe offsets (dy, dx) from the frame's top-left, 0x08172DB7. Small players use entries
/// 0x16 - 2k, big 6 - 2k (k = 0..3); +8 selects the right-hand side probes.
const PROBES: [(u8, u8); 16] = [
    (32, 8), (5, 8), (24, 3), (12, 3), (32, 8), (5, 8), (24, 13), (12, 13),
    (32, 8), (18, 8), (24, 3), (23, 3), (32, 8), (18, 8), (24, 13), (23, 13),
];
const FRAME_W: i32 = 16;
const FRAME_H: i32 = 32;

/// Points the ROM tests for coins this frame, in level pixels.
fn probes(p: &Player, frame: u32) -> [(i32, i32); 4] {
    let base: usize = match p.form {
        Form::Small => 0x16,
        Form::Big | Form::Raccoon => 6,
    };
    // Right-hand probes when moving right; standing still alternates sides every frame.
    let right = if p.vx == 0 { frame & 1 == 0 } else { p.vx > 0 };
    let base = base + if right { 8 } else { 0 };
    let left = p.centre().0 - FRAME_W / 2;
    let top = p.px().1 + p.height() - FRAME_H;
    std::array::from_fn(|k| {
        let (dy, dx) = PROBES[(base - 2 * k) / 2];
        (left + dx as i32, top + dy as i32)
    })
}

/// Collects coins under the player's probes; returns the collected cells.
pub fn collect(level: &mut Level, p: &Player, frame: u32) -> Vec<(i32, i32)> {
    let blank = level.rules.final_metatile[0];
    let mut got = Vec::new();
    for (x, y) in probes(p, frame) {
        let (tx, ty) = (x.div_euclid(TILE_PX), y.div_euclid(TILE_PX));
        if level.metatile(tx, ty) == Some(COIN) {
            level.set(tx, ty, blank);
            got.push((tx, ty));
        }
    }
    got
}

/// Sprite offsets (dx, dy) of the four sparkle pieces, 0x08170CDA / 0x08170CDE.
pub const SPARKLE_OFFSETS: [(i32, i32); 4] = [(-2, 4), (4, 14), (10, 4), (4, -6)];
/// Ticks until the sparkle ends.
const SPARKLE_TICKS: u8 = 11;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sparkle {
    /// Top-left of the collected cell, pixels.
    pub x: i32,
    pub y: i32,
    pub counter: u8,
}

impl Sparkle {
    pub fn new(tx: i32, ty: i32) -> Self {
        Self { x: tx * TILE_PX, y: ty * TILE_PX, counter: 0 }
    }

    /// One frame. Returns false when it ends.
    pub fn step(&mut self, frame: u32) -> bool {
        if frame & 1 != 0 {
            self.counter += 1;
        }
        self.counter < SPARKLE_TICKS
    }

    /// Visible pieces: (x, y, frame) with frame 0 = tile 0x68 (smallest) .. 4 = 0x6C.
    pub fn pieces(&self) -> impl Iterator<Item = (i32, i32, usize)> + '_ {
        SPARKLE_OFFSETS.iter().enumerate().filter_map(|(k, &(dx, dy))| {
            // Piece k shows tiles 0x6C..0x68 on ticks 2k..2k+4 (0x08170CE2).
            let t = self.counter as i32 - 2 * k as i32;
            (0..5).contains(&t).then(|| (self.x + dx, self.y + dy, (4 - t) as usize))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::{SKY, TEST_SOLIDITY};
    use sma4_assets::BlockRules;

    fn level_with_coin() -> Level {
        let (w, h) = (10, 6);
        let mut m = vec![SKY; w * h];
        m[3 * w + 4] = COIN;
        let mut rules = BlockRules::default();
        rules.final_metatile[0] = 0x41;
        Level::new(w, h, m, TEST_SOLIDITY, rules)
    }

    #[test]
    fn touching_a_coin_collects_it_once() {
        let mut lvl = level_with_coin();
        let p = Player::spawn(4 * TILE_PX + 2, 3 * TILE_PX);
        assert_eq!(collect(&mut lvl, &p, 0), vec![(4, 3)]);
        assert_eq!(lvl.metatile(4, 3), Some(0x41));
        assert!(collect(&mut lvl, &p, 0).is_empty());
    }

    #[test]
    fn coin_out_of_reach_stays() {
        let mut lvl = level_with_coin();
        let p = Player::spawn(7 * TILE_PX, 3 * TILE_PX);
        assert!(collect(&mut lvl, &p, 0).is_empty());
        assert_eq!(lvl.metatile(4, 3), Some(COIN));
    }

    #[test]
    fn sparkle_plays_staggered_pieces_then_ends() {
        let mut s = Sparkle::new(1, 1);
        assert_eq!(s.pieces().collect::<Vec<_>>(), vec![(14, 20, 4)]);
        let mut frames = 0;
        let mut f = 0;
        while s.step(f) {
            f += 1;
            frames += 1;
        }
        // Ticks on odd frames: the 11th tick comes on frame 21.
        assert_eq!(frames, 21);
        s.counter = 8;
        assert_eq!(s.pieces().map(|p| p.2).collect::<Vec<_>>(), vec![0, 2]);
    }
}
