//! The mini-fortress's hazards that are not enemies one can stomp (see docs/re-notes.md,
//! "Mini-fortress"):
//!
//! * Podoboo (type 0x9E, init 0x080550BC, main 0x0805548C): spawned 8 px right of its list
//!   cell, it remembers that y as home (+0x3A/+0x3B) and drops into the lava below. Its feet
//!   probe (0x08045ABC, class 0: 16 px down, 3 px across) finding the lava top (tile 0x94)
//!   hides it for (random & 0x3F) | 0x40 frames (+0x23), with a splash (projectile 0x18);
//!   then it jumps with speed 0x0816E5F8[(y - home) >> 4] (negative: that speed; positive:
//!   speed -0x80 with gravity held off for that many frames, +0x34), falling back with +2 a
//!   frame up to 0x70. Rising fast (speed below -0x20) it leaves embers (projectile 0x17)
//!   every 4 frames. It hurts on any touch; the star knocks it out unless it is hidden.
//! * Roto-Disc (types 0x5A / 0x5B, init 0x08051034 / 0x08051058 -> 0x08050F80, main
//!   0x0805166C): starts 0x30 px left of its pivot block with speed byte -0x28 / +0x28 up
//!   and down, 3 px down / 2 px up; every other frame (0x0805152C, mode +0x3B = 6) each speed
//!   steps by 2 toward +/-0x28 (0x0816E4CC / 0x0816E4DB) and turns there, so it circles
//!   the block (0x5A one way, 0x5B the other). Any touch hurts, the star does nothing.
//!
//! Positions in 1/256 px, speeds in 1/16 px per frame (the ROM's speed bytes).

use crate::level::{Level, TILE_PX};
use crate::player::SUB;
use sma4_assets::EnemySpawn;

pub const PODOBOO: u16 = 0x9E;
pub const ROTO_DISC: u16 = 0x5A;
pub const ROTO_DISC_CCW: u16 = 0x5B;
/// Camera limits (init 0x0805D7C0 / 0x0805D808, then removed): 0xC7 stops the camera
/// with the screen's right edge at its column (max camera x = x - 0xF0, if the camera is
/// not already past it); 0xEA keeps it from scrolling left of its column (min camera x = x,
/// once the camera is at or right of it).
pub const CAMERA_MAX: u16 = 0xC7;
pub const CAMERA_MIN: u16 = 0xEA;
const CAMERA_MAX_OFFSET: i32 = 0xF0;
/// Treasure chest (init 0x08069A00, main 0x08069DBC); list type 0x1AA's row byte is its
/// item (0x0300379D: 0x0C = Warp Whistle); type 0x166 sets the level effect 7
/// (0x03003CF7, 0x0804D12C) that ends the level 0x60 frames after the chest is emptied.
pub const CHEST: u16 = 0x52;
pub const CHEST_ITEM: u16 = 0x1AA;
pub const CHEST_EXIT: u16 = 0x166;

/// The enemy-list types this module (and `crate::boom_boom`) handles.
pub const OBJECTS: [u16; 9] =
    [PODOBOO, ROTO_DISC, ROTO_DISC_CCW, crate::boom_boom::BOOM_BOOM, CAMERA_MAX, CAMERA_MIN, CHEST, CHEST_ITEM, CHEST_EXIT];

/// A camera limit entry coming up: (min, max) camera x after it, given the current limits.
pub fn camera_limit(s: &EnemySpawn, camera_x: i32, limits: (Option<i32>, Option<i32>)) -> (Option<i32>, Option<i32>) {
    let x = s.col as i32 * TILE_PX;
    match s.kind {
        CAMERA_MAX if x - CAMERA_MAX_OFFSET >= camera_x => (limits.0, Some(x - CAMERA_MAX_OFFSET)),
        CAMERA_MIN if x <= camera_x => (Some(x), limits.1),
        _ => limits,
    }
}

/// Lava top in the fortress tileset (feet probe code 0x94).
pub const LAVA: u16 = 0x94;
/// Podoboo init: x + 8 px.
const PODOBOO_X: i32 = 8;
/// Feet probe 0x08169E78[0] while falling: (dy 16, dx 3).
const PODOBOO_FEET: (i32, i32) = (3, 16);
/// 0x0816E5F8: jump speed by depth below home in tiles (negative), or frames of no gravity
/// with speed -0x80 (positive).
const PODOBOO_JUMP: [i8; 32] = [
    -16, -36, -48, -56, -64, -72, -78, -84, -90, -96, -102, -106, -110, -116, -120, -124, -128, 4, 8, 12, 16, 20, 0, 1,
    0, 1, -128, -127, -128, -127, 0, 1,
];
const PODOBOO_GRAVITY: i32 = 2;
const PODOBOO_MAX_FALL: i32 = 0x70;
/// Hidden in the lava for (random & 0x3F) | 0x40 frames.
const PODOBOO_WAIT_MASK: u8 = 0x3F;
const PODOBOO_WAIT_MIN: u8 = 0x40;
/// Embers while rising faster than this (0x080553F0), one every 4 frames (+0x1D).
const EMBER_SPEED: i32 = -0x20;
const EMBER_EVERY: u8 = 4;
const EMBER_DY: i32 = 0x0E;
/// Projectile 0x17: three steps of 4 frames.
pub const EMBER_STEPS: u8 = 3;
const EMBER_STEP: u8 = 4;
/// Projectile 0x18: 0x18 frames.
pub const SPLASH_FRAMES: u8 = 0x18;

/// Roto-Disc init (0x08050F80): x - 0x30 px, y + 3 px (0x5A) / - 2 px (0x5B).
const DISC_X: i32 = -0x30;
const DISC_Y: [i32; 2] = [3, -2];
const DISC_SPEED: i32 = 0x28;
/// Mode 6 of 0x0805152C: steps of 2 every other frame (mask 0x0816E4D7[3] = 1).
const DISC_STEP: i32 = 2;
const DISC_MASK: u8 = 1;

/// Contact box of these objects (+0x64 & 0x3F = 1, as the Goomba's): dx 2, w 12, dy 2,
/// h 14 against the player's box.
const BOX: (i32, i32, i32, i32) = (2, 12, 2, 14);

fn overlaps((ex, ey): (i32, i32), (x, y, w, h): (i32, i32, i32, i32)) -> bool {
    let (dx, bw, dy, bh) = BOX;
    ex + dx < x + w && x < ex + dx + bw && ey + dy < y + h && y < ey + dy + bh
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Podoboo {
    pub x: i32,
    pub y: i32,
    /// Speed byte +0x12.
    pub vy: i32,
    /// +0x3A/+0x3B, px.
    home: i32,
    /// Hidden in the lava while this runs (+0x23).
    pub timer: u8,
    /// Ember cooldown +0x1D.
    ember_wait: u8,
    /// Frames without gravity after a deep jump (+0x34).
    float: u8,
    /// Animation frame +0x17 (0-2).
    pub frame: u8,
    /// Knocked out by the star: falls off upside down.
    pub dead: bool,
}

/// What a Podoboo did this frame.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PodobooEvents {
    pub ember: Option<(i32, i32)>,
    pub splash: Option<(i32, i32)>,
    /// Sound 0x8E: it jumps out of the lava.
    pub jumped: bool,
}

impl Podoboo {
    pub fn spawn(s: &EnemySpawn) -> Self {
        let y = s.row as i32 * TILE_PX;
        Self {
            x: (s.col as i32 * TILE_PX + PODOBOO_X) * SUB,
            y: y * SUB,
            vy: 0,
            home: y,
            timer: 0,
            ember_wait: 0,
            float: 0,
            frame: 0,
            dead: false,
        }
    }

    pub fn px(&self) -> (i32, i32) {
        (self.x >> 8, self.y >> 8)
    }

    /// Hidden in the lava (+0x64 | 0x500): not drawn, not touched.
    pub fn hidden(&self) -> bool {
        self.timer != 0
    }

    /// Falling (+0x1A bit 7: drawn upside down).
    pub fn falling(&self) -> bool {
        self.vy >= 0
    }

    /// One frame (0x0805548C, after the object loop counted +0x23 / +0x34 down). `rand` is
    /// the slot's random byte (0x03003879[slot]).
    pub fn step(&mut self, level: &Level, frame: u32, rand: u8) -> PodobooEvents {
        let mut ev = PodobooEvents::default();
        if self.dead {
            self.vy = (self.vy + 3).min(0x40);
            self.y += self.vy * 16;
            return ev;
        }
        self.timer = self.timer.saturating_sub(1);
        self.float = self.float.saturating_sub(1);
        self.ember_wait = self.ember_wait.saturating_sub(1);
        if self.timer == 0 && self.ember_wait == 0 && self.vy < EMBER_SPEED {
            self.ember_wait = EMBER_EVERY;
            ev.ember = Some((self.px().0 + (rand & 7) as i32, self.px().1 + EMBER_DY));
        }
        if self.timer != 0 {
            // 0x0805548C: the frame it comes out, the splash and sound 0x8E.
            if self.timer == 1 {
                ev.splash = Some(self.px());
                ev.jumped = true;
            }
            return ev;
        }
        // 0x080552D4: frame +0x17 steps every 4 frames through 0, 1, 2.
        if frame & 3 == 0 {
            self.frame = (self.frame + 1) % 3;
        }
        let (x, y) = self.px();
        if self.vy >= 0 && level.metatile((x + PODOBOO_FEET.0).div_euclid(TILE_PX), (y + PODOBOO_FEET.1).div_euclid(TILE_PX)) == Some(LAVA) {
            let c = PODOBOO_JUMP[((y - self.home) >> 4 & 0x1F) as usize];
            if c < 0 {
                self.vy = c as i32;
            } else {
                self.float = c as u8;
                self.vy = -0x80;
            }
            ev.splash = Some((x, y));
            self.timer = (rand & PODOBOO_WAIT_MASK) | PODOBOO_WAIT_MIN;
            return ev;
        }
        // 0x08055208: move, then gravity unless floating.
        self.y += self.vy * 16;
        if self.float == 0 && self.vy < PODOBOO_MAX_FALL {
            self.vy += PODOBOO_GRAVITY;
        }
        ev
    }

    /// Touches the player's box `(x, y, w, h)`.
    pub fn touches(&self, b: (i32, i32, i32, i32)) -> bool {
        !self.hidden() && !self.dead && overlaps(self.px(), b)
    }

    /// The star knocks it out (0x080488E4): speed 0xC8 up.
    pub fn knock_out(&mut self) {
        self.dead = true;
        self.vy = -0x38;
    }
}

/// Podoboo embers (projectile 0x17, 0x0808F6E4): an 8x8 OBJ in three steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ember {
    pub x: i32,
    pub y: i32,
    pub step: u8,
    timer: u8,
}

impl Ember {
    pub fn new((x, y): (i32, i32)) -> Self {
        Self { x, y, step: 0, timer: EMBER_STEP }
    }

    pub fn step(&mut self) -> bool {
        self.timer -= 1;
        if self.timer == 0 {
            self.timer = EMBER_STEP;
            self.step += 1;
        }
        self.step < EMBER_STEPS
    }
}

/// Lava splash (projectile 0x18, 0x0808F7E4): four 8x8 pieces spreading out, 0x18 frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LavaSplash {
    pub x: i32,
    pub y: i32,
    pub timer: u8,
}

/// Splash pieces by step (timer >> 2): outer / inner x offsets, outer / inner y offsets
/// (0x08170B75, 0x08170B7B, 0x08170B81, 0x08170B87) and the tile (0x08170B8E).
const SPLASH_OUTER_X: [i32; 7] = [15, 14, 13, 12, 11, 10, 18];
const SPLASH_INNER_X: [i32; 6] = [18, 16, 14, 12, 10, 8];
const SPLASH_OUTER_Y: [i32; 6] = [10, 9, 9, 10, 11, 12];
const SPLASH_INNER_Y: [i32; 6] = [6, 5, 4, 6, 8, 10];
pub const SPLASH_TILE_STEPS: usize = 6;

impl LavaSplash {
    pub fn new((x, y): (i32, i32)) -> Self {
        Self { x, y, timer: SPLASH_FRAMES }
    }

    /// 0x0808FB08 counts +0x15 down, then 0x0808F7E4 ends the splash at 0.
    pub fn step(&mut self) -> bool {
        self.timer -= 1;
        self.timer != 0
    }

    /// Its step (0..5), which picks the tiles.
    pub fn index(&self) -> usize {
        (self.timer >> 2) as usize
    }

    /// The four pieces' top-lefts (the right two drawn h-flipped).
    pub fn pieces(&self) -> [(i32, i32); 4] {
        let i = self.index();
        let s = self.x + 4;
        let (ox, ix) = (SPLASH_OUTER_X[i], SPLASH_INNER_X[i.min(5)]);
        let (oy, iy) = (self.y + SPLASH_OUTER_Y[i.min(5)], self.y + SPLASH_INNER_Y[i.min(5)]);
        [(s - ox, oy), (s - ix, iy), (s + ox, iy), (s + ix, oy)]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RotoDisc {
    pub x: i32,
    pub y: i32,
    vx: i32,
    vy: i32,
    /// +0x3E / +0x3F: which way each speed is heading.
    turn: [u8; 2],
    /// +0x3C, +1 a frame.
    count: u8,
}

impl RotoDisc {
    pub fn spawn(s: &EnemySpawn) -> Self {
        let ccw = (s.kind == ROTO_DISC_CCW) as usize;
        Self {
            x: (s.col as i32 * TILE_PX + DISC_X) * SUB,
            y: (s.row as i32 * TILE_PX + DISC_Y[ccw]) * SUB,
            vx: 0,
            vy: if ccw == 1 { DISC_SPEED } else { -DISC_SPEED },
            turn: [0; 2],
            count: 0,
        }
    }

    pub fn px(&self) -> (i32, i32) {
        (self.x >> 8, self.y >> 8)
    }

    /// One frame of 0x0805152C (mode 6).
    pub fn step(&mut self) {
        self.x += self.vx * 16;
        self.y += self.vy * 16;
        self.count = self.count.wrapping_add(1);
        if self.count & DISC_MASK == 0 {
            let [tx, ty] = &mut self.turn;
            for (v, turn) in [(&mut self.vx, tx), (&mut self.vy, ty)] {
                if *v == if *turn == 0 { DISC_SPEED } else { -DISC_SPEED } {
                    *turn ^= 1;
                }
                *v += if *turn == 0 { DISC_STEP } else { -DISC_STEP };
            }
        }
    }

    pub fn touches(&self, b: (i32, i32, i32, i32)) -> bool {
        overlaps(self.px(), b)
    }
}

/// Boom Boom's burst (projectile 0x0D, 0x0808E480): eight sparks flying out, slower as
/// they age (they move when frame & 0x08170B2A[(life >> 4) & 3] = 7, 3, 1, 0 is 0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spark {
    pub x: i32,
    pub y: i32,
    vx: i32,
    vy: i32,
    pub life: u8,
}

const SPARK_MASKS: [u32; 4] = [7, 3, 1, 0];

impl Spark {
    /// The eight sparks from Boom Boom at `(x, y)` (1/256 px): from (x + 4, y + 16), speeds
    /// 0x0816F1C0[i] down and [(i + 2) & 7] across.
    pub fn burst(x: i32, y: i32) -> Vec<Self> {
        let t = crate::boom_boom::SPARK_SPEEDS;
        (0..8)
            .map(|i| Self { x: x + 4 * SUB, y: y + 16 * SUB, vy: t[i], vx: t[(i + 2) & 7], life: crate::boom_boom::SPARK_LIFE })
            .collect()
    }

    pub fn px(&self) -> (i32, i32) {
        (self.x >> 8, self.y >> 8)
    }

    pub fn step(&mut self, frame: u32) -> bool {
        self.life -= 1;
        if self.life == 0 {
            return false;
        }
        if SPARK_MASKS[((self.life >> 4) & 3) as usize] & frame == 0 {
            self.x += self.vx * 16;
            self.y += self.vy * 16;
        }
        true
    }
}

/// A puff of smoke (projectile 0x16, 0x0808F568): two 8x16 OBJs, tile 0x08170B64[life >> 3]
/// (0x83, 0x82, 0x81), 0x17 frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Puff {
    pub x: i32,
    pub y: i32,
    pub life: u8,
}

pub const PUFF_LIFE: u8 = 0x17;

impl Puff {
    pub fn new((x, y): (i32, i32)) -> Self {
        Self { x, y, life: PUFF_LIFE }
    }

    pub fn step(&mut self) -> bool {
        self.life -= 1;
        self.life != 0
    }

    /// 0 (big) .. 2 (small).
    pub fn index(&self) -> usize {
        2 - (self.life >> 3) as usize
    }
}

/// The treasure chest (type 0x52).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chest {
    pub x: i32,
    pub y: i32,
    vy: i32,
    /// Appearing (+0x23 = 0x1F; the puff and sound 0x6D at 0x18).
    pub appear: u8,
    /// Opened: the item rises (speed 0xD0 + 1 a frame), then shows for 0x70 frames (+0x24).
    pub item: Option<u8>,
    pub show: u8,
}

/// What the chest did this frame.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ChestEvents {
    pub puff: Option<(i32, i32)>,
    pub opened: bool,
    /// The item went into the inventory; the level ends 0x60 frames later.
    pub got: Option<u8>,
}

const CHEST_APPEAR: u8 = 0x1F;
const CHEST_PUFF_AT: u8 = 0x18;
const ITEM_RISE: i32 = -0x30;
const ITEM_SHOW: u8 = 0x70;
/// Level effect 7: frames from the item to the end of the level (+0xBE9 = 0x60).
pub const CHEST_EXIT_FRAMES: u8 = 0x60;

/// Level effect 7's chest (0x0804D12C): object slot 0, y 0x170; x the camera + 0xC0, or +
/// 0x30 with the player at or right of x 0x80.
pub const CHEST_DROP_Y: i32 = 0x170;
pub const CHEST_SIDE: i32 = 0x80;

impl Chest {
    pub fn spawn(s: &EnemySpawn) -> Self {
        Self::at(s.col as i32 * TILE_PX, s.row as i32 * TILE_PX)
    }

    /// A chest appearing at pixel (x, y).
    pub fn at(x: i32, y: i32) -> Self {
        Self { x: x * SUB, y: y * SUB, vy: 0, appear: CHEST_APPEAR, item: None, show: 0 }
    }

    pub fn px(&self) -> (i32, i32) {
        (self.x >> 8, self.y >> 8)
    }

    /// Visible: not while it appears, not once the item is gone.
    pub fn visible(&self) -> bool {
        self.appear == 0
    }

    /// The item shows (it blinks: +0x24 & 0xC).
    pub fn item_shown(&self) -> Option<u8> {
        let item = self.item?;
        (self.show == 0 || self.show & 0xC != 0).then_some(item)
    }

    /// One frame of 0x08069DBC; `item` is 0x0300379D, `player` the player's box.
    pub fn step(&mut self, level: &Level, item: u8, player: (i32, i32, i32, i32)) -> ChestEvents {
        let mut ev = ChestEvents::default();
        if self.appear != 0 {
            self.appear -= 1;
            if self.appear == CHEST_PUFF_AT {
                ev.puff = Some(self.px());
            }
            return ev;
        }
        if self.item.is_none() {
            // 0x08069920: fall and land.
            self.y += self.vy * 16;
            let (px, py) = self.px();
            if level.at((px + 8).div_euclid(TILE_PX), (py + 16).div_euclid(TILE_PX)) != sma4_assets::TileKind::Empty && self.vy >= 0 {
                self.y &= !0xFFF;
                self.vy = 0;
            } else {
                self.vy = (self.vy + 3).min(0x40);
            }
            if overlaps(self.px(), player) {
                // Sounds 0x6E / 0x6F; the clock stops; the item rises out (speed 0xD0).
                self.item = Some(item);
                self.vy = ITEM_RISE;
                ev.opened = true;
                ev.puff = Some(self.px());
            }
            return ev;
        }
        // 0x08069AC8.
        if self.show == 0 {
            self.y += self.vy * 16;
            self.vy += 1;
            if self.vy >= 0 {
                self.show = ITEM_SHOW;
            }
        } else {
            self.show -= 1;
            if self.show == 1 {
                ev.got = self.item;
            }
        }
        ev
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::SKY;

    fn lava_pit() -> Level {
        let (w, h) = (8, 27);
        let mut m = vec![SKY; w * h];
        for x in 0..w {
            m[25 * w + x] = LAVA;
        }
        Level::new(w, h, m, Default::default(), Default::default())
    }

    #[test]
    fn a_podoboo_drops_into_the_lava_waits_and_jumps_back_to_its_home() {
        let level = lava_pit();
        let mut p = Podoboo::spawn(&EnemySpawn { kind: PODOBOO, col: 2, row: 19, ..Default::default() });
        assert_eq!(p.px(), (40, 304));
        let mut f = 0;
        while !p.hidden() {
            f += 1;
            p.step(&level, f, 0x2B);
        }
        assert_eq!(p.px().1, 0x183, "lands in the lava (ROM: y 0x183 after 37 frames)");
        assert_eq!(f, 38);
        assert_eq!(p.vy, -72, "0x0816E5F8[5]");
        let mut top = p.px().1;
        for _ in 0..200 {
            f += 1;
            p.step(&level, f, 0x2B);
            top = top.min(p.px().1);
        }
        assert_eq!(top, 304, "back up to its home");
    }

    #[test]
    fn touching_the_chest_opens_it_and_the_item_rises_then_is_got() {
        let level = lava_pit();
        let mut c = Chest::spawn(&EnemySpawn { kind: CHEST, col: 3, row: 23, ..Default::default() });
        let away = (200, 0, 12, 16);
        let mut puffs = 0;
        for _ in 0..CHEST_APPEAR {
            puffs += c.step(&level, 0x0C, away).puff.is_some() as u32;
        }
        assert_eq!(puffs, 1, "it appears in a puff");
        assert!(c.visible());
        let (x, y) = c.px();
        assert!(c.step(&level, 0x0C, (x + 2, y, 12, 16)).opened);
        let mut got = None;
        for _ in 0..200 {
            got = got.or(c.step(&level, 0x0C, away).got);
        }
        assert_eq!(got, Some(0x0C), "the Warp Whistle");
        assert!(c.px().1 < y, "it rose out of the chest");
    }

    #[test]
    fn a_roto_disc_circles_its_pivot() {
        let mut d = RotoDisc::spawn(&EnemySpawn { kind: ROTO_DISC, col: 10, row: 10, ..Default::default() });
        let start = d.px();
        let (mut min_x, mut max_x) = (i32::MAX, i32::MIN);
        for _ in 0..400 {
            d.step();
            min_x = min_x.min(d.px().0);
            max_x = max_x.max(d.px().0);
        }
        assert_eq!(start, (160 - 0x30, 163));
        assert!(max_x - min_x > 60, "swings right across the pivot");
    }
}
