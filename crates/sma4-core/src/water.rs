//! Water effects (World 1-5's pools): the splash where something goes into or out of the
//! water (effect slots 0x03007848 + 0x888, update 0x0808A5B8, draw 0x0808A128) and air
//! bubbles (slots + 0x858, spawned by 0x08089B4C and 0x0809F4C8, update 0x08089E9C).

use crate::level::{Level, TILE_PX};
use crate::player::{Form, Player};
use sma4_assets::TileKind;

/// Splash slots: the player's is the first; objects take the other two in turn (0xC94).
pub const SPLASH_SLOTS: usize = 3;
/// The splash goes once its timer passes this (0x0808A5B8).
const SPLASH_END: u8 = 0x2F;
/// From this step on the splash sinks 1 px every other frame (0x0808A128).
const SPLASH_SINK_STEP: u8 = 3;

/// One splash: 16x16 at (x, y), step `timer >> 3` of `sma4_assets::splash_path`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Splash {
    pub x: i32,
    pub y: i32,
    pub timer: u8,
    pub slot: usize,
}

impl Splash {
    /// The player's splash (0x0809F4C8): on the 16 px row of the head (frame y + 10 small,
    /// + 0 otherwise) + 2, or the row below when that one is open air.
    pub fn player(player: &Player, level: &Level) -> Self {
        let (fx, fy) = player.frame();
        let dy = if player.form == Form::Small { 10 } else { 0 };
        let mut y = ((fy + dy) & !0xF) + 2;
        let below = ((fx + 8).div_euclid(TILE_PX), (y + 0x10).div_euclid(TILE_PX));
        if y + 0x10 <= level.pixel_height() && level.at(below.0, below.1) == TileKind::Empty && !level.is_water(below.0, below.1)
        {
            y += 0x10;
        }
        Self { x: fx, y, timer: 1, slot: 0 }
    }

    /// An object's splash (0x0804A224): x, and its y less 4 rounded down to the row, + 2.
    pub fn object(x: i32, y: i32, slot: usize) -> Self {
        Self { x, y: ((y - 4) & !0xF) + 2, timer: 1, slot }
    }

    pub fn step_index(&self) -> usize {
        (self.timer >> 3) as usize
    }

    /// 0x0808A5B8: +1 a frame, +2 on odd frames; gone past 0x2F. The draw then sinks the
    /// later steps by 1 px on even frames.
    pub fn step(&mut self, frame: u32) -> bool {
        self.timer += if frame & 1 != 0 { 2 } else { 1 };
        if self.timer > SPLASH_END {
            return false;
        }
        if self.timer >> 3 >= SPLASH_SINK_STEP && frame & 1 == 0 {
            self.y += 1;
        }
        true
    }
}

/// Bubble slots: three for the player (0x08089B4C / 0x0809F4C8 take them from the last
/// down), one more for objects falling in.
pub const BUBBLE_SLOTS: usize = 4;
const PLAYER_BUBBLE_SLOTS: usize = 3;
/// 0x08089B4C: a breath every 0x80 frames, 0x40 while A, left or right is held.
const BREATH_EVERY: u32 = 0x80;
const BREATH_EVERY_MOVING: u32 = 0x40;
/// Where the breath leaves the mouth in the swimming poses (0x0813F07C -> 0x08170906 /
/// 0x081707B4: x 1 facing left, 0xD - 1 facing right; y 16, small 22).
const MOUTH_X: (i32, i32) = (1, 12);
const MOUTH_Y: (i32, i32) = (16, 22);
/// Going in (0x0809F4C8): one bubble per slot at these offsets from the frame.
const DIVE_Y: [i32; 3] = [16, 22, 19];
const DIVE_X: [i32; 3] = [0, 4, 11];
/// Sideways sway by (spin counter >> 2) & 3 (0x081709AF).
const SWAY: [i32; 4] = [0, 1, 0, -1];

/// One air bubble: an 8x8 OBJ at (x - sway, y).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bubble {
    pub x: i32,
    pub y: i32,
    /// +8: counts up with bit 7 set; it rises while bits 4-5 are not both clear.
    counter: u8,
    pub slot: usize,
}

impl Bubble {
    /// Draw x for this frame.
    pub fn draw_x(&self, frame: u32) -> i32 {
        self.x - SWAY[((frame >> 2) & 3) as usize]
    }

    /// 0x08089E9C: rise, and pop once out of the water.
    pub fn step(&mut self, level: &Level) -> bool {
        let c = self.counter.wrapping_add(1);
        self.counter = c | 0x80;
        if c & 0x30 != 0 {
            self.y -= 1;
        }
        level.is_water(self.x.div_euclid(TILE_PX), ((self.y + 2) & !0xF).div_euclid(TILE_PX))
    }
}

fn free_slot(bubbles: &[Bubble]) -> Option<usize> {
    (0..PLAYER_BUBBLE_SLOTS).rev().find(|s| !bubbles.iter().any(|b| b.slot == *s))
}

/// 0x08089B4C: the player breathes out a bubble now and then.
pub fn breathe(bubbles: &mut Vec<Bubble>, player: &Player, moving: bool, frame: u32) {
    let every = if moving { BREATH_EVERY_MOVING } else { BREATH_EVERY };
    if frame & (every - 1) != 0 {
        return;
    }
    let Some(slot) = free_slot(bubbles) else { return };
    let (fx, fy) = player.frame();
    let dx = if player.facing_left { MOUTH_X.0 } else { MOUTH_X.1 };
    let dy = if player.form == Form::Small { MOUTH_Y.1 } else { MOUTH_Y.0 };
    bubbles.push(Bubble { x: fx + dx, y: fy + dy, counter: 0x40, slot });
}

/// 0x0809F4C8 (going in while falling): a bubble in each free player slot, with a random
/// start (0x03003879[slot] | 0x10).
pub fn dive(bubbles: &mut Vec<Bubble>, player: &Player, random: impl Fn(usize) -> u8) {
    let (fx, fy) = player.frame();
    for slot in (0..PLAYER_BUBBLE_SLOTS).rev() {
        if !bubbles.iter().any(|b| b.slot == slot) {
            bubbles.push(Bubble { x: fx + DIVE_X[slot], y: fy + DIVE_Y[slot], counter: random(slot) | 0x10, slot });
        }
    }
}

/// 0x0804A224 (an object falling in): a bubble in the last slot if free, 6 px down and 4
/// px in.
pub fn object_dive(bubbles: &mut Vec<Bubble>, x: i32, y: i32) {
    let slot = BUBBLE_SLOTS - 1;
    if !bubbles.iter().any(|b| b.slot == slot) {
        bubbles.push(Bubble { x: x + 4, y: y + 6, counter: 0x80, slot });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::test_pool;

    #[test]
    fn a_splash_runs_six_steps_and_sinks_in_the_last_three() {
        let mut s = Splash::object(32, 100, 1);
        assert_eq!((s.y, s.step_index()), (((100 - 4) & !0xF) + 2, 0));
        let mut frames = 0;
        let mut steps = vec![];
        while s.step(frames) {
            steps.push(s.step_index());
            frames += 1;
        }
        // +1 / +2 a frame from 1 to past 0x2F: 32 frames.
        assert_eq!(frames, 31);
        assert_eq!(*steps.last().unwrap(), 5);
        assert!(s.y > ((100 - 4) & !0xF) + 2);
    }

    #[test]
    fn bubbles_rise_and_pop_at_the_surface() {
        let level = test_pool();
        let mut b = Bubble { x: 40, y: 5 * TILE_PX + 8, counter: 0x80, slot: 0 };
        let mut n = 0;
        while b.step(&level) {
            n += 1;
            assert!(n < 200);
        }
        // The pool's surface row is 4: the bubble pops once (y + 2) is above row 5.
        assert!(b.y + 2 < 5 * TILE_PX);
    }
}
