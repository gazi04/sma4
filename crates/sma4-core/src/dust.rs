//! Skid dust: the puffs left behind while the player skids (holding against the motion on
//! the ground).
//!
//! * 0x08097800 (ground pose): when the skid pose is picked and both dust slots are free,
//!   slot 0 (0x03003890) = 0x10 at the player's position (x 0x03003894, y 0x0300389C) with
//!   the player's facing (0x030038A4, 0x40 = facing right).
//! * 0x0808C270, later in the frame: each live slot (high nibble = puff number 1-4, low =
//!   age) ages by one. At age 4, while the player still skids, the other slot starts again
//!   where the player is now, its own number + 1 (at most 4); at age 8 a slot ends. So a puff
//!   comes out every 4 frames for as long as the skid lasts, numbered 1, 1, 2, 2, 3, 3, 4 ...
//! * Draw: one 8x8 OBJ per slot, tile 0x08170A88[age >> 1], palette 0, at x +
//!   0x08170A80[facing right * 4 + number - 1], y + 0x1A.

use crate::player::Player;

/// Puff x offsets from the player's frame by number, facing left / right (0x08170A80).
const OFFSETS: [[i32; 4]; 2] = [[-1, 2, 5, 8], [8, 5, 2, -1]];
/// Below the top of the player's frame (0x0808C270).
const DY: i32 = 0x1A;
/// A slot's age when the next puff starts, and when the slot ends.
const NEXT_AGE: u8 = 4;
const END_AGE: u8 = 8;
const MAX_PUFF: u8 = 4;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Slot {
    /// 0 = free; high nibble = puff number, low nibble = age.
    state: u8,
    x: i32,
    y: i32,
    facing_right: bool,
}

impl Slot {
    fn start(&mut self, state: u8, p: &Player) {
        let (x, y) = p.frame();
        *self = Self { state, x, y, facing_right: !p.facing_left };
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SkidDust {
    slots: [Slot; 2],
}

impl SkidDust {
    /// One frame, after the player has moved.
    pub fn step(&mut self, p: &Player) {
        let free = self.slots.iter().all(|s| s.state == 0);
        if !p.skidding && free {
            return;
        }
        if p.skidding && free {
            self.slots[0].start(0x10, p);
        }
        for i in [1, 0] {
            if self.slots[i].state == 0 {
                continue;
            }
            self.slots[i].state += 1;
            let age = self.slots[i].state & 0xF;
            if age == NEXT_AGE && p.skidding {
                let other = &mut self.slots[i ^ 1];
                let number = ((other.state >> 4) + 1).min(MAX_PUFF);
                other.start(number << 4, p);
            } else if age == END_AGE {
                self.slots[i].state = 0;
            }
        }
    }

    /// The puffs to draw: top-left of each 8x8 sprite (level pixels) and its cell of
    /// `sma4_assets::skid_dust_path`.
    pub fn puffs(&self) -> impl Iterator<Item = (i32, i32, usize)> + '_ {
        self.slots.iter().rev().filter(|s| s.state != 0).map(|s| {
            let number = (s.state >> 4).clamp(1, MAX_PUFF) as usize;
            let dx = OFFSETS[s.facing_right as usize][number - 1];
            (s.x + dx, s.y + DY, ((s.state & 0xF) >> 1) as usize)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn puffs_alternate_slots_while_skidding_then_fade() {
        let mut p = Player::default();
        p.skidding = true;
        p.facing_left = true;
        let mut d = SkidDust::default();
        let (fx, fy) = p.frame();
        let mut starts = Vec::new();
        let mut before = d.slots;
        for f in 0..20 {
            d.step(&p);
            for (i, (now, old)) in d.slots.iter().zip(before).enumerate() {
                if now.state >> 4 != old.state >> 4 {
                    starts.push((f, i, now.state >> 4));
                }
            }
            before = d.slots;
        }
        // A slot restarting mid-frame is drawn at age 0 the same frame (0x0808C270), so
        // the slots take turns 3 and 4 frames apart; each counts its own puffs up to 4.
        assert_eq!(
            starts,
            [(0, 0, 1), (3, 1, 1), (7, 0, 2), (10, 1, 2), (14, 0, 3), (17, 1, 3)]
        );
        let xs: Vec<_> = d.puffs().map(|(x, y, _)| (x - fx, y - fy)).collect();
        assert_eq!(xs, [(5, DY), (5, DY)], "puff 3 facing left");

        p.skidding = false;
        let mut frames = 0;
        while d.puffs().next().is_some() {
            d.step(&p);
            frames += 1;
        }
        assert!(frames <= 8, "the last puffs live out their 8 frames");
    }

    #[test]
    fn no_dust_without_a_skid() {
        let mut d = SkidDust::default();
        d.step(&Player::default());
        assert_eq!(d.puffs().count(), 0);
    }
}
