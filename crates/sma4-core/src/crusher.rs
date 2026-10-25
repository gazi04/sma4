//! The mini-fortress's crushing ceiling (autoscroll kind 3: enemy-list type 0x1A7 with row
//! 0x3s, start 0x0804B0A4, each frame 0x08092BF0 -> 0x0809290C). See docs/re-notes.md,
//! "Crushing ceiling".
//!
//! * The screen is split: lines from 0x7F down show the room's floor where it is, the lines
//!   above scroll with +0xBB4, which swings between 0x0817121E[dir | script << 1] (script 0:
//!   0x9C going up, 0x10F going down), starting at the camera's y. So the spiked ceiling
//!   comes down to 0x74 px lower and goes back up.
//! * Speed +0xBBA steps by 0x0817121A[dir] (-1 / +1) every 8 frames up to 0x0817121C[dir]
//!   (-8 / +8, half a pixel a frame). At the end it stops, turns, waits 0x20 frames (+0xBBE,
//!   shaking a pixel every other two frames) and, having come down, thuds (sound 0x6B, a
//!   rumble).
//! * Probes of the player and objects below screen line 0x7F read the fixed floor
//!   (0x08099FB8, 0x08045FE4 with 0x03003D07); everything else sees the moving part.
//! * 0x08092030 each frame: the tile at the player's frame + (8, 0x12 small / 6 big) in the
//!   moving part being the class's hurt tile (0x08172E36[class] = 0x66) or 0x08172E1A[class]
//!   (0xE3) or one less (the spikes) hurts the player (0x0804A5F4).
//! * Doors only work while it is within 3 px of its top (0x0809588C).
//!
//! Here the room keeps its starting camera and the part above the split is drawn and
//! collides `shift()` px lower (`Level::shift`).

/// End positions by direction (script 0), the speed steps and their limits.
const ENDS: [i32; 2] = [0x9C, 0x10F];
const ACCEL: [i32; 2] = [-1, 1];
const MAX_SPEED: [i32; 2] = [-8, 8];
const PAUSE: u8 = 0x20;
/// Screen line where the fixed floor starts (0x9F - 0x20).
pub const SPLIT: i32 = 0x80;
/// Head probe below the frame's top: small (or crouching) 0x12, big 6.
pub const HEAD_SMALL: i32 = 0x12;
pub const HEAD_BIG: i32 = 6;
/// Hurt tiles of the fortress class (1).
pub const HURT_TILE: u8 = 0x66;
pub const SPIKES: u8 = 0xE3;
/// Doors work within this many px of the top position.
const DOOR_SLACK: i32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Crusher {
    /// +0xBB4, 1/256 px.
    y: i32,
    /// +0xBBA, 1/16 px per frame.
    speed: i32,
    /// +0xBC1: 0 going up (the ceiling comes down), 1 going down.
    dir: usize,
    /// +0xBBE.
    pause: u8,
    /// The room's camera y, where the floor stays.
    base: i32,
}

impl Crusher {
    /// Start (0x0804B0A4): at the camera's y, at rest, heading up.
    pub fn start(camera_y: i32) -> Self {
        Self { y: camera_y << 8, speed: 0, dir: 0, pause: 0, base: camera_y }
    }

    /// A second entry restarts it from where it is (+0xBB0.. cleared, +0xBB4 = camera y).
    pub fn restart(&mut self) {
        let y = self.y >> 8 << 8;
        *self = Self { y, ..Self::start(self.base) };
    }

    /// The room's camera y (the floor's), kept while the crusher runs.
    pub fn base(&self) -> i32 {
        self.base
    }

    /// How far down the ceiling has come, px.
    pub fn shift(&self) -> i32 {
        self.base - (self.y >> 8)
    }

    /// Fixed-frame screen line from which the floor is fixed (`Level::shift`'s split).
    pub fn split_y(&self) -> i32 {
        self.base + SPLIT
    }

    pub fn door_open(&self) -> bool {
        ((self.y >> 8) - ENDS[1]).abs() <= DOOR_SLACK
    }

    /// One frame (0x08092BF0 -> 0x0809290C). Returns true when it thuds at the bottom.
    pub fn step(&mut self, frame: u32) -> bool {
        // 0x08092BF0 counts the pause down first; while it runs the ceiling shakes
        // (0x080928E8: y ^= (pause & 1) << 8, a pixel).
        if self.pause != 0 {
            self.pause -= 1;
            if self.pause != 0 {
                if self.pause & 1 != 0 {
                    self.y ^= 0x100;
                }
                return false;
            }
        }
        self.y += self.speed * 16;
        if frame & 7 == 0 && self.speed != MAX_SPEED[self.dir] {
            self.speed += ACCEL[self.dir];
        }
        if self.y >> 8 != ENDS[self.dir] {
            return false;
        }
        self.speed = 0;
        self.dir ^= 1;
        self.pause = PAUSE;
        self.dir != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_comes_down_0x74_px_waits_and_goes_back_up() {
        let mut c = Crusher::start(0x110);
        let (mut max, mut thuds) = (0, 0);
        for f in 0..2000 {
            if c.step(f) {
                thuds += 1;
            }
            max = max.max(c.shift());
        }
        assert_eq!(max, 0x110 - 0x9C);
        assert!(thuds >= 2);
    }
}
