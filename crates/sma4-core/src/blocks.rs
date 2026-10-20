//! Hit blocks and the items they release, reimplemented from the ROM's routines
//! (see docs/re-notes.md, "Blocks and items"):
//!
//! * head hit -> behaviour (0x0809FC1C, handlers at 0x0817310C) -> hit result
//! * bump-block object, type 6 (init 0x08084EB8, main 0x08084FBC): 10-frame bounce,
//!   spawns the item on its first frame, writes the final tile on its last
//! * growth item, object type 0x0D (init 0x0804E480, main 0x0804EA38): waits, rises out of
//!   the block, then walks away from the player and turns at walls
//! * Super Leaf, object type 0x1E (init 0x0804E5A0, main 0x0804EBE4): pops up, then sways
//!   down like a pendulum, ignoring the terrain

use sma4_assets::TileKind;

use crate::level::{Level, TILE_PX};
use crate::player::{Form, HITBOX_W, Player, SUB};

/// Metatile left in the cell while the block bounces: solid, drawn by the bump sprite.
const BUMP_PLACEHOLDER: u16 = 0xF3;
const BUMP_FRAMES: u8 = 10;

/// Object type of the growth item spawned for hit result 5.
pub const GROWTH_ITEM: u16 = 0x0D;
/// Object type of the Super Leaf (hit result 3: a big player hits the block).
pub const LEAF: u16 = 0x1E;
/// Fire Flower (hit result 2, init 0x0804E548, main 0x0804EB00): rises out of the block
/// and stays put (its speed is cleared while it is not falling).
pub const FIRE_FLOWER: u16 = 0x19;
/// Starman (hit result 4, init 0x0804E4E0, main 0x0804E984): rises out of the block, then
/// bounces along away from the player.
pub const STAR: u16 = 0x0C;
/// Starman speeds (1/16 px per frame): across 0x10 away from the player (0x0804E0F4); out
/// of a block hit from below it jumps at 0xC8 (-0x38), and every landing 0xC0 (-0x40).
const STAR_SPEED: i32 = 0x10 * 16;
const STAR_LAUNCH: i32 = -0x38 * 16;
const STAR_BOUNCE: i32 = -0x40 * 16;
/// Flower and star can't be taken for 0x10 frames (+0x24).
const ITEM_GRACE: u8 = 0x10;

// Leaf (speeds in 1/16 px per frame). A head hit sets 0x03003CFB = 1, so it starts at
// 0x0816E3A4[1] = -32 and rises while the speed climbs by 1 per frame to 0. Then it
// sways: horizontal speed changes by 0x0816E3AC[dir] = +/-2 per frame until it reaches
// 0x0816E3AE[dir] = +/-32, where dir flips; vertical speed is 0x0816E3B0[dir + (vx < 0)]
// + 6 = 16 / -4 / 14. The player can't take it for its first 16 frames (+0x24).
const LEAF_POP: i32 = -32;
const LEAF_SWAY_ACCEL: [i32; 2] = [2, -2];
const LEAF_SWAY_MAX: [i32; 2] = [32, -32];
const LEAF_SWAY_FALL: [i32; 3] = [16, -4, 14];
const LEAF_GRACE: u8 = 0x10;

// Item timings from 0x0804E118: emerge timer starts at 0x3D, rising begins below 0x2D and
// moves 1 px every 3 frames (sub-counter reloads with 2).
const EMERGE_FRAMES: u8 = 0x3D;
const RISE_BELOW: u8 = 0x2D;
const RISE_EVERY: u8 = 3;
/// Horizontal speed byte 0x10 (1/16 px per frame) = 1 px per frame.
const ITEM_SPEED: i32 = 0x10 * 16;
// Shared object gravity (0x08049EC0): speed byte += 0x08169D9E[0] = 3 each frame, capped
// at 0x08169D9C[0] = 64 while falling (1/16 px per frame; water uses 1 and 16, not ported).
const ITEM_GRAVITY: i32 = 3 * 16;
const ITEM_MAX_FALL: i32 = 64 * 16;
const ITEM_SIZE: i32 = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bump {
    pub tx: i32,
    pub ty: i32,
    /// Counts down from 10 to 1, like the ROM's per-slot timer at 0x03003CC0.
    pub timer: u8,
    /// Upward displacement of the bouncing block, 1/256 px.
    pub lift: i32,
    pub result: u8,
    /// Metatile the cell turns into when the bounce ends.
    pub final_metatile: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemPhase {
    /// Inside the block; `timer` counts down from `EMERGE_FRAMES`.
    Emerging { timer: u8, sub: u8 },
    Walking,
    /// Leaf popping up out of the block.
    Popping,
    /// Leaf swaying down; `dir` is the current swing (0 right, 1 left).
    Swaying { dir: u8 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Item {
    pub kind: u16,
    /// Top-left, 1/256 px.
    pub x: i32,
    pub y: i32,
    pub vx: i32,
    pub vy: i32,
    pub phase: ItemPhase,
    /// Frames left before the player can take it.
    pub grace: u8,
}

impl Item {
    pub fn px(&self) -> (i32, i32) {
        (self.x.div_euclid(SUB), self.y.div_euclid(SUB))
    }
}

/// Coin that pops out of a hit coin block (slots at 0x03007848 + 0x8AC..0x8DC; init
/// 0x0808A62C, update 0x0808A974): starts 24 px above the block moving up 4 px/frame, the
/// speed grows by 1 every 4 frames, and the coin vanishes once it would exceed 4 px/frame
/// falling (35 frames: up 36 px, ends 4 px below its start).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoinPop {
    /// Top-left in pixels (16x16 box over the block's column).
    pub x: i32,
    pub y: i32,
    /// Pixels per frame, negative = up.
    pub vy: i8,
    counter: u8,
}

impl CoinPop {
    pub fn new(tx: i32, ty: i32) -> Self {
        Self { x: tx * TILE_PX, y: ty * TILE_PX - 24, vy: -4, counter: 1 }
    }

    /// Spin frame 0..3 (draw routine 0x0808A7F8: counter >> 2 & 3).
    pub fn frame(&self) -> usize {
        ((self.counter >> 2) & 3) as usize
    }

    /// One frame. Returns false when the coin is gone.
    pub fn step(&mut self) -> bool {
        self.counter = self.counter.wrapping_add(1);
        self.y += self.vy as i32;
        if self.counter & 3 == 0 {
            let old = self.vy;
            self.vy += 1;
            if old == 4 {
                return false;
            }
        }
        true
    }
}

/// What a hit on a block produced, for counters and sound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitOutcome {
    Nothing,
    Coin,
    Item,
}

/// Reacts to the player's head hitting tile `(tx, ty)`. Returns `None` if the tile does
/// not react (a plain ceiling).
pub fn hit_block(level: &mut Level, player: &Player, tx: i32, ty: i32) -> Option<(Bump, HitOutcome)> {
    let m = level.metatile(tx, ty)?;
    if m >= 0x100 {
        return None; // high-layer specials use a different path (0x080A12BC), not ported
    }
    let behaviour = level.rules.bump_behaviour[m as usize];
    // Behaviour handlers (table 0x0817310C) return the low nibble of the hit result.
    let small = player.form == Form::Small;
    let (low, outcome) = match behaviour {
        // 0x0809FF34: mushroom for a small player, else the fire flower (result 2).
        1 => (if small { 5 } else { 2 }, HitOutcome::Item),
        // 0x0809FF64: power-up block. Small player gets result 5, otherwise 3.
        2 => (if small { 5 } else { 3 }, HitOutcome::Item),
        // 0x0809FF94: star (result 4, 0x03003D04 = 0x80).
        3 => (4, HitOutcome::Item),
        // 0x0809FFAC: coin block (0x080A03A8 gives the coin), result 1.
        4 => (1, HitOutcome::Coin),
        // TODO(re): other behaviours (bricks, vines, multi-coin...).
        _ => return None,
    };
    let result = low | level.rules.result_hi[m as usize];
    let kind = level.rules.final_kind[(result >> 4) as usize];
    let final_metatile = match kind {
        0 => m,
        // Kind 2 restores the tileset's empty tile (0x03003DCA = sky).
        2 => crate::level::SKY,
        k => level.rules.final_metatile[(k - 1) as usize],
    };
    level.set(tx, ty, BUMP_PLACEHOLDER);
    let bump = Bump { tx, ty, timer: BUMP_FRAMES, lift: 0, result, final_metatile };
    Some((bump, outcome))
}

impl Bump {
    /// One frame of the bump-block object. Returns an item to spawn, and whether it ended.
    pub fn step(&mut self, level: &mut Level, player: &Player) -> (Option<Item>, bool) {
        let mut spawned = None;
        if self.timer == BUMP_FRAMES {
            let kind = level.rules.item_types[(self.result & 0xF) as usize];
            if matches!(kind, GROWTH_ITEM | LEAF | FIRE_FLOWER | STAR) {
                spawned = Some(Item::spawn(kind, self.tx, self.ty, player));
            }
            // TODO(re): other item types (1-up, vine...).
        }
        if self.timer == 1 {
            level.set(self.tx, self.ty, self.final_metatile);
            return (spawned, true);
        }
        // Bounce speed per timer value (1/16 px per frame). Positive values move the block
        // up first, then back down; over timers 10..2 the profile sums to zero.
        self.lift += level.rules.bounce[self.timer as usize] as i32 * 16;
        self.timer -= 1;
        (spawned, false)
    }
}

impl Item {
    fn spawn(kind: u16, tx: i32, ty: i32, player: &Player) -> Self {
        let (x, y) = (tx * TILE_PX, ty * TILE_PX);
        // 0x0804E08C: moves right when the player is at or left of the item.
        let dir = if player.px().0 <= x { 1 } else { -1 };
        if kind == LEAF {
            return Self {
                kind,
                x: x * SUB,
                y: y * SUB,
                vx: 0,
                vy: LEAF_POP * 16,
                phase: ItemPhase::Popping,
                grace: LEAF_GRACE,
            };
        }
        let (vx, vy, grace) = match kind {
            FIRE_FLOWER => (0, 0, ITEM_GRACE),
            STAR => (dir * STAR_SPEED, STAR_LAUNCH, ITEM_GRACE),
            _ => (dir * ITEM_SPEED, 0, 0),
        };
        Self { kind, x: x * SUB, y: y * SUB, vx, vy, phase: ItemPhase::Emerging { timer: EMERGE_FRAMES, sub: 0 }, grace }
    }

    /// Leaf (0x0804EBE4), speeds in 1/16 px per frame.
    fn leaf(&mut self) {
        self.grace = self.grace.saturating_sub(1);
        let (mut vx, mut vy) = (self.vx / 16, self.vy / 16);
        if self.phase == ItemPhase::Popping {
            self.y += vy * 16;
            vy += 1;
            if vy != 0 {
                self.vy = vy * 16;
                return;
            }
            self.phase = ItemPhase::Swaying { dir: 0 };
        }
        let ItemPhase::Swaying { dir } = &mut self.phase else { return };
        let d = (*dir & 1) as usize;
        vx += LEAF_SWAY_ACCEL[d];
        if vx == LEAF_SWAY_MAX[d] {
            *dir = dir.wrapping_add(1);
        }
        vy = LEAF_SWAY_FALL[d + (vx < 0) as usize];
        self.vx = vx * 16;
        self.vy = vy * 16;
        self.x += self.vx;
        self.y += self.vy;
    }

    /// One frame. Returns false when the item left the level.
    pub fn step(&mut self, level: &Level) -> bool {
        match &mut self.phase {
            ItemPhase::Emerging { timer, sub } => {
                if *timer < RISE_BELOW {
                    if *sub == 0 {
                        *sub = RISE_EVERY - 1;
                        self.y -= SUB;
                    } else {
                        *sub -= 1;
                    }
                }
                *timer -= 1;
                if *timer == 0 {
                    self.phase = ItemPhase::Walking;
                }
                true
            }
            ItemPhase::Walking => {
                self.grace = self.grace.saturating_sub(1);
                let landed = self.walk(level);
                // 0x0804E984: the star bounces off the ground.
                if landed && self.kind == STAR {
                    self.vy = STAR_BOUNCE;
                }
                self.px().1 < level.pixel_height() + TILE_PX
            }
            ItemPhase::Popping | ItemPhase::Swaying { .. } => {
                self.leaf();
                self.px().1 < level.pixel_height() + TILE_PX
            }
        }
    }

    /// Returns true if it is standing on the ground.
    fn walk(&mut self, level: &Level) -> bool {
        // Horizontal: turn around at walls.
        self.x += self.vx;
        let (px, py) = self.px();
        let rows = (py + 2).div_euclid(TILE_PX)..=(py + ITEM_SIZE - 2).div_euclid(TILE_PX);
        let front = if self.vx > 0 { px + ITEM_SIZE - 1 } else { px };
        let tx = front.div_euclid(TILE_PX);
        if rows.into_iter().any(|ty| level.at(tx, ty) == TileKind::Solid) {
            self.x = if self.vx > 0 { (tx * TILE_PX - ITEM_SIZE) * SUB } else { (tx + 1) * TILE_PX * SUB };
            self.vx = -self.vx;
        }
        // Vertical: fall, land on solid or top tiles.
        let old_bottom = py + ITEM_SIZE;
        self.vy = (self.vy + ITEM_GRAVITY).min(ITEM_MAX_FALL);
        self.y += self.vy;
        let (px, py) = self.px();
        let ty = (py + ITEM_SIZE).div_euclid(TILE_PX);
        let top = ty * TILE_PX;
        let cols = (px + 2).div_euclid(TILE_PX)..=(px + ITEM_SIZE - 3).div_euclid(TILE_PX);
        let lands = cols.into_iter().any(|tx| match level.at(tx, ty) {
            TileKind::Solid => true,
            TileKind::Top => old_bottom <= top,
            TileKind::Empty => false,
        });
        if lands && self.vy >= 0 {
            self.y = (top - ITEM_SIZE) * SUB;
            self.vy = 0;
            return true;
        }
        false
    }

    /// Whether the item's box overlaps the player's.
    pub fn touches(&self, player: &Player) -> bool {
        if matches!(self.phase, ItemPhase::Emerging { .. }) || self.grace > 0 {
            return false;
        }
        let (ix, iy) = self.px();
        let (px, py) = player.px();
        ix < px + HITBOX_W && px < ix + ITEM_SIZE && iy < py + player.height() && py < iy + ITEM_SIZE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::{SKY, TEST_SOLIDITY};
    use sma4_assets::BlockRules;

    const QBLOCK: u16 = 0x61;
    const COIN_BLOCK: u16 = 0x63;
    const USED: u16 = 0x5F;
    const GROUND: u16 = 0x53;

    /// Rules with the values the extractor reads from the European ROM for tileset 1.
    fn rules() -> BlockRules {
        let mut r = BlockRules::default();
        r.bump_behaviour[QBLOCK as usize] = 2;
        r.bump_behaviour[COIN_BLOCK as usize] = 4;
        r.result_hi[QBLOCK as usize] = 0x20;
        r.result_hi[COIN_BLOCK as usize] = 0x20;
        r.item_types[5] = GROWTH_ITEM;
        r.item_types[3] = 0x1E;
        r.final_kind[2] = 8;
        r.final_metatile[7] = USED;
        r.bounce = [0, -64, -64, -48, -32, -16, 0, 16, 32, 48, 64];
        r
    }

    /// 20x8 level, ground on the last row, a block at (5, 4).
    fn level(block: u16) -> Level {
        let (w, h) = (20, 8);
        let mut m = vec![SKY; w * h];
        for x in 0..w {
            m[(h - 1) * w + x] = GROUND;
        }
        m[4 * w + 5] = block;
        Level::new(w, h, m, TEST_SOLIDITY, rules())
    }

    #[test]
    fn question_block_releases_growth_item_and_becomes_used() {
        let mut lvl = level(QBLOCK);
        let player = Player::spawn(4 * TILE_PX, 6 * TILE_PX - 16);
        let (mut bump, outcome) = hit_block(&mut lvl, &player, 5, 4).unwrap();
        assert_eq!(outcome, HitOutcome::Item);
        assert_eq!(bump.result, 0x25);
        assert_eq!(lvl.metatile(5, 4), Some(BUMP_PLACEHOLDER));

        let (item, done) = bump.step(&mut lvl, &player);
        let item = item.expect("growth item spawned on the first bump frame");
        assert_eq!(item.kind, GROWTH_ITEM);
        assert!(!done);
        assert!(bump.lift > 0, "block moves up first");
        let mut frames = 1;
        while !bump.step(&mut lvl, &player).1 {
            frames += 1;
        }
        assert_eq!(frames + 1, BUMP_FRAMES as usize);
        assert_eq!(bump.lift, 0, "block returns to its cell");
        assert_eq!(lvl.metatile(5, 4), Some(USED));
        assert_eq!(lvl.at(5, 4), TileKind::Solid);
    }

    #[test]
    fn big_player_gets_a_different_result() {
        let mut lvl = level(QBLOCK);
        let mut player = Player::spawn(4 * TILE_PX, 6 * TILE_PX - 16);
        player.grow();
        let (bump, _) = hit_block(&mut lvl, &player, 5, 4).unwrap();
        assert_eq!(bump.result, 0x23);
    }

    #[test]
    fn coin_block_gives_coin_and_no_item() {
        let mut lvl = level(COIN_BLOCK);
        let player = Player::spawn(4 * TILE_PX, 6 * TILE_PX - 16);
        let (mut bump, outcome) = hit_block(&mut lvl, &player, 5, 4).unwrap();
        assert_eq!(outcome, HitOutcome::Coin);
        assert!(bump.step(&mut lvl, &player).0.is_none());
    }

    #[test]
    fn coin_pop_rises_falls_and_disappears() {
        let mut c = CoinPop::new(5, 4);
        let start = c.y;
        let (mut frames, mut top) = (0, c.y);
        while c.step() {
            frames += 1;
            top = top.min(c.y);
        }
        assert_eq!(frames + 1, 35);
        assert_eq!(start - top, 36, "rises 36 px");
        assert_eq!(c.y - start, 4, "ends slightly below where it started");
        assert_eq!(start, 4 * TILE_PX - 24, "starts 24 px above the block");
    }

    #[test]
    fn plain_tiles_do_not_react() {
        let mut lvl = level(GROUND);
        let player = Player::spawn(4 * TILE_PX, 6 * TILE_PX - 16);
        assert!(hit_block(&mut lvl, &player, 5, 4).is_none());
    }

    #[test]
    fn item_rises_out_of_the_block_then_walks_away_from_player() {
        let mut lvl = level(QBLOCK);
        let player = Player::spawn(4 * TILE_PX, 6 * TILE_PX - 16); // left of the block
        let (mut bump, _) = hit_block(&mut lvl, &player, 5, 4).unwrap();
        let mut item = bump.step(&mut lvl, &player).0.unwrap();
        while !bump.step(&mut lvl, &player).1 {}
        let start_y = item.px().1;
        // Waits 16 frames, then rises 1 px every 3 frames.
        for _ in 0..(EMERGE_FRAMES - RISE_BELOW) {
            item.step(&lvl);
        }
        assert_eq!(item.px().1, start_y);
        while matches!(item.phase, ItemPhase::Emerging { .. }) {
            item.step(&lvl);
        }
        assert_eq!(start_y - item.px().1, 15);
        // Walks right (away from the player) at 1 px/frame, then falls to the ground.
        let x0 = item.px().0;
        for _ in 0..10 {
            item.step(&lvl);
        }
        assert_eq!(item.px().0 - x0, 10);
        for _ in 0..200 {
            item.step(&lvl);
        }
        assert_eq!(item.px().1 + ITEM_SIZE, 7 * TILE_PX, "landed on the ground");
    }

    #[test]
    fn item_turns_at_walls() {
        let mut lvl = level(QBLOCK);
        lvl.set(12, 6, GROUND);
        let player = Player::spawn(4 * TILE_PX, 6 * TILE_PX - 16);
        let mut item = Item {
            kind: GROWTH_ITEM,
            x: 8 * TILE_PX * SUB,
            y: 6 * TILE_PX * SUB,
            vx: ITEM_SPEED,
            vy: 0,
            phase: ItemPhase::Walking,
            grace: 0,
        };
        for _ in 0..80 {
            item.step(&lvl);
        }
        assert!(item.vx < 0);
        let _ = player;
    }
}
