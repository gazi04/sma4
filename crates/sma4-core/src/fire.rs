//! Fire Mario's fireballs, reimplemented from the ROM (see docs/re-notes.md, "Fire Mario"):
//!
//! * B (0x0808C068) as fire takes the first free of two slots (0x03007848 + 0xAE8, then
//!   + 0xAD4; 0x0808BFEC) and 0x0808BECC throws: 1 px (facing left) / 7 px (right) into the
//!   player's frame, 5 px below its top, speed 0x30 down and 0x30 across, sound 0x59; the
//!   player shows the throw pose for 0xB frames.
//! * 0x0808B9F4 each frame: y += vy, x += vx (1/16 px); every 4th frame vy += 0x10, up to
//!   0x40. Off the screen sideways it is gone; below the view gone, above it flies on
//!   without touching anything.
//! * 0x0808B028: a solid tile ahead (x, or x + 8 moving right; y + 8) puffs it out
//!   (0x0808AE98, sound 0x60). The floor under (x, y + 0xE) bounces it back up at 0x30
//!   (0x0808AA6C): a top-solid tile only within its top 5 px; a solid one pushes it up 2 px
//!   first, and the second solid tile in a row puffs it. In a slope tileset the top-solid
//!   tiles follow their height map (0x0808AB50): bounce at 0x50 going uphill, 0x20 going
//!   down.
//! * Enemies (0x0808B648 -> 0x0808B2F0) are checked by the game (`Enemy::fireball_hit`).
//! * Puff (type 3..0x10, 0x0808BC0C): 13 frames, tiles 0x08170A03[type >> 2].

use sma4_assets::TileKind;

use crate::level::{Level, TILE_PX};
use crate::player::{Player, SUB};

/// Two slots (0x0808BFEC searches +0xAE8 then +0xAD4).
pub const FIREBALL_SLOTS: usize = 2;
const SPEED_X: i32 = 0x30;
const SPEED_Y: i32 = 0x30;
const FALL_STEP: i32 = 0x10;
const MAX_FALL: i32 = 0x40;
/// Bounce speeds 0x081709E6: flat, steep (into a slope), shallow (down a slope).
const BOUNCE: [i32; 3] = [-0x30, -0x50, -0x20];
/// Spawn offsets into the player's 16x32 frame (0x0808BECC).
const THROW_DX: [i32; 2] = [1, 7];
const THROW_DY: i32 = 5;
const FLOOR_PROBE: i32 = 0xE;
const WALL_PROBE: i32 = 8;
/// A top-solid tile only stops it within its top 5 px.
const TOP_DEPTH: i32 = 4;
/// Puff: type 3 counts up to 0x10.
const PUFF_FIRST: u8 = 3;
const PUFF_END: u8 = 0x10;

/// One of Fire Mario's fireballs (player projectile type 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerFireball {
    /// Top-left, 1/256 px (+0 / +4).
    pub x: i32,
    pub y: i32,
    /// Speeds in 1/16 px per frame (+0x0A / +0x08).
    pub vx: i32,
    vy: i32,
    /// +0x0F, +1 a frame (set to 3 by a bounce).
    counter: u8,
    /// +0x0E: solid floor tiles in a row.
    floor_hits: u8,
    /// Type 3.. once it has burst (0x0808AA64), up to 0x10.
    pub puff: u8,
}

/// What a frame did to a fireball.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FireStep {
    Flying,
    /// Burst against a wall (sound 0x60): show the puff.
    Burst,
    Gone,
}

impl PlayerFireball {
    /// 0x0808BECC.
    pub fn thrown(player: &Player) -> Self {
        let (fx, fy) = player.frame();
        let right = !player.facing_left;
        let sub = player.x.rem_euclid(SUB);
        Self {
            x: (fx + THROW_DX[right as usize]) * SUB + sub,
            y: (fy + THROW_DY) * SUB + player.y.rem_euclid(SUB),
            vx: if right { SPEED_X } else { -SPEED_X },
            vy: SPEED_Y,
            counter: 0,
            floor_hits: 0,
            puff: 0,
        }
    }

    pub fn px(&self) -> (i32, i32) {
        (self.x >> 8, self.y >> 8)
    }

    /// Whether it can still hit things (not bursting).
    pub fn live(&self) -> bool {
        self.puff == 0
    }

    /// Spin frame (the draw 0x0808B6A0 uses the frame counter 0x03003CF6 >> 2): a cell of
    /// `sma4_assets::fireball_path`, flipped when flying left.
    pub fn frame(&self, frame: u32) -> usize {
        ((frame >> 2) & 3) as usize
    }

    /// Puff cell of `sma4_assets::puff_path` (tiles 0x82, 0x80, 0x81, 0x83 in turn).
    pub fn puff_cell(&self) -> usize {
        const CELLS: [usize; 4] = [2, 1, 0, 3];
        CELLS[((self.puff >> 2) & 3) as usize]
    }

    fn burst(&mut self) -> FireStep {
        self.puff = PUFF_FIRST;
        FireStep::Burst
    }

    /// One frame. `camera` is the view's top-left.
    pub fn step(&mut self, level: &Level, camera: (i32, i32)) -> FireStep {
        if self.puff != 0 {
            self.puff += 1;
            return if self.puff == PUFF_END { FireStep::Gone } else { FireStep::Flying };
        }
        self.y += self.vy * 16;
        self.counter = self.counter.wrapping_add(1);
        self.x += self.vx * 16;
        if self.counter & 3 == 0 {
            self.vy += FALL_STEP;
        }
        if self.vy > MAX_FALL - 1 {
            self.vy = MAX_FALL;
        }
        let (x, y) = self.px();
        let sx = x - camera.0;
        if !(-0x10..0x100).contains(&sx) {
            return FireStep::Gone;
        }
        let sy = y - camera.1;
        if !(-0x20..0xC0).contains(&sy) {
            return if sy >= 0 { FireStep::Gone } else { FireStep::Flying };
        }
        self.collide(level)
    }

    /// 0x0808B028.
    fn collide(&mut self, level: &Level) -> FireStep {
        let (x, y) = self.px();
        let tile = |px: i32, py: i32| (px.div_euclid(TILE_PX), py.div_euclid(TILE_PX));
        let wall_x = if self.vx < 0 { x } else { x + WALL_PROBE };
        let (tx, ty) = tile(wall_x, y + WALL_PROBE);
        if level.at(tx, ty) == TileKind::Solid {
            return self.burst();
        }
        let feet = y + FLOOR_PROBE;
        let (tx, ty) = tile(x, feet);
        let depth = feet.rem_euclid(TILE_PX);
        match level.at(tx, ty) {
            TileKind::Empty => self.floor_hits = 0,
            TileKind::Top if level.has_slopes() => return self.slope_bounce(level, tx, ty, x, depth),
            TileKind::Top => {
                if depth <= TOP_DEPTH {
                    self.bounce(BOUNCE[0]);
                }
            }
            TileKind::Solid => {
                if level.has_slopes() && level.collision_id(tx, ty).is_some_and(|id| SLOPE_GROUND.contains(&id)) {
                    self.bounce(BOUNCE[0]);
                    return FireStep::Flying;
                }
                self.floor_hits += 1;
                if self.floor_hits == 2 {
                    return self.burst();
                }
                self.y -= 2 * SUB;
                self.bounce(BOUNCE[0]);
            }
        }
        FireStep::Flying
    }

    /// 0x0808AB50: a top-solid tile of a slope tileset.
    fn slope_bounce(&mut self, level: &Level, tx: i32, ty: i32, x: i32, depth: i32) -> FireStep {
        let shape = level.slope(tx, ty);
        let Some(dir) = crate::enemies::slope_dir(shape) else {
            return self.burst();
        };
        if depth > 0xB || level.slope_height(shape, x) <= depth {
            let speed = match dir {
                0 => BOUNCE[0],
                d if (self.vx >> 4 ^ d as i32) < 0 => BOUNCE[1],
                _ => BOUNCE[2],
            };
            self.bounce(speed);
        }
        FireStep::Flying
    }

    /// 0x0808AA6C.
    fn bounce(&mut self, speed: i32) {
        self.vy = speed;
        self.counter = 3;
    }

    /// Burst on an enemy (0x0808B2F0 -> 0x0808AA64).
    pub fn hit(&mut self) {
        self.puff = PUFF_FIRST;
    }
}

/// Solid ids that are the ground under a slope (0x0808B028 lets them bounce the fireball
/// like a floor in slope tilesets).
const SLOPE_GROUND: [u8; 3] = [0x37, 0xAB, 0xF9];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player::Form;

    /// 30x10: floor on row 9, a wall at column 12.
    fn level() -> Level {
        let (w, h) = (30, 10);
        let mut kinds = vec![TileKind::Empty; w * h];
        for x in 0..w {
            kinds[9 * w + x] = TileKind::Solid;
        }
        for y in 0..9 {
            kinds[y * w + 12] = TileKind::Solid;
        }
        Level::from_kinds(w, h, &kinds)
    }

    fn fire_player(x: i32) -> Player {
        let mut p = Player::spawn(x, 9 * TILE_PX - 26);
        p.form = Form::Fire;
        p
    }

    #[test]
    fn thrown_from_the_frame_and_bouncing_along_the_floor_until_the_wall() {
        let p = fire_player(48);
        let (fx, fy) = p.frame();
        let mut b = PlayerFireball::thrown(&p);
        assert_eq!(b.px(), (fx + 7, fy + 5));
        let mut lowest = 0;
        let mut bounces = 0;
        let mut last_vy = b.vy;
        let mut step = FireStep::Flying;
        for _ in 0..200 {
            step = b.step(&level(), (0, 0));
            if step != FireStep::Flying {
                break;
            }
            lowest = lowest.max(b.px().1);
            if b.vy < 0 && last_vy >= 0 {
                bounces += 1;
            }
            last_vy = b.vy;
        }
        assert_eq!(step, FireStep::Burst, "bursts on the wall");
        assert!(bounces >= 2, "bounced {bounces} times");
        assert!(lowest + FLOOR_PROBE <= 9 * TILE_PX + 4, "never sinks into the floor: {lowest}");
        assert!(b.px().0 + WALL_PROBE >= 12 * TILE_PX);
        let mut n = 0;
        while b.step(&level(), (0, 0)) != FireStep::Gone {
            n += 1;
        }
        assert_eq!(n + 1, (PUFF_END - PUFF_FIRST) as usize, "the puff lasts 13 frames");
    }

    #[test]
    fn it_flies_left_when_the_player_faces_left_and_leaves_the_screen() {
        let mut p = fire_player(100);
        p.facing_left = true;
        let mut b = PlayerFireball::thrown(&p);
        assert!(b.vx < 0);
        assert_eq!(b.px().0, p.frame().0 + 1);
        let mut frames = 0;
        while b.step(&level(), (100, 0)) == FireStep::Flying {
            frames += 1;
            assert!(frames < 100);
        }
        assert!(b.px().0 < 100 - 0x10 + 3);
    }
}
