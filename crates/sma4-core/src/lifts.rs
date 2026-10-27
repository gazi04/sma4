//! World 1-4's falling lifts (object type 0x36, init 0x0806B9CC, main 0x0806B9DC; see
//! docs/re-notes.md, "Falling lift"):
//!
//! * Init: x speed +0x10 = 0xF8 (left at 1/2 px a frame); y is nudged up (0x0806B82C: -2,
//!   or -1 when its pixel part is not a multiple of 256), so it lies 1 px above its row.
//! * Each frame (0x0806B9DC): y += y speed (capped at 0x40 while falling, 0x08049EA0), x
//!   += x speed (0x08049DE0), then the player contact 0x0806B530. Until someone stands on
//!   it the y speed stays 0; the first frame the player stands on it, it stops moving
//!   across, and from then on its y speed grows by 1 every frame.
//! * Contact (0x0806B530, box 0x08169F44[0x0816A516[0x36] & 0x3F] = (2, 43, -2, 18)):
//!   from above (the player's 32 px frame at least 0x18 px above it, not rising) the
//!   player stands on it, its frame 0x1F px above the lift, and moves across with it;
//!   from the side, 8 px or more below its top (+0x10 for a small or crouching player), a
//!   player pushing against a lift that is not moving across stops, and one in front of a
//!   moving lift is carried at its speed; from below, a rising player is sent down
//!   (speed byte 0x10, sound 0x60).
//! * Off the screen it is removed (0x0804A1EC), so it does not come back.
//!
//! Positions in 1/256 px, speeds in 1/16 px per frame (the ROM's speed bytes).

use sma4_assets::EnemySpawn;
use crate::level::TILE_PX;
use crate::player::{Form, HITBOX_W, Player, SUB, UNIT};

pub const LIFT: u16 = 0x36;
const DRIFT: i32 = -8;
const MAX_FALL: i32 = 0x40;
/// Box (dx, w, dy, h), px.
const BOX: (i32, i32, i32, i32) = (2, 43, -2, 18);
/// Standing: the frame is this far above the lift.
const STAND_OFFSET: i32 = 0x1F;
/// The frame must be more than this above the lift's top to land on it.
const LAND_ABOVE: i32 = 0x18;
/// Below this far under the top it is a side contact (+0x10 small or crouching).
const SIDE_BELOW: i32 = 8;
const SIDE_BELOW_SMALL: i32 = 0x10;
/// A head hit sends the player down at this speed byte.
const HEAD_PUSH: i32 = 0x10;
/// Removed this far off the screen (0x08169DA8: x > camera + 0x120, x < camera - 0x40).
const OFFSCREEN_RIGHT: i32 = 0x120;
const OFFSCREEN_LEFT: i32 = 0x40;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lift {
    pub x: i32,
    pub y: i32,
    vx: i32,
    vy: i32,
}

/// What the lift did to the player this frame.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Contact {
    /// The player stands on it.
    pub standing: bool,
    /// The player's head hit it.
    pub head: bool,
}

impl Lift {
    pub fn spawn(s: &EnemySpawn) -> Self {
        let y = s.row as i32 * TILE_PX * SUB;
        let nudge = if y & 0xFF00 == 0 { 2 } else { 1 };
        Self { x: s.col as i32 * TILE_PX * SUB, y: y - nudge, vx: DRIFT, vy: 0 }
    }

    pub fn px(&self) -> (i32, i32) {
        (self.x >> 8, self.y >> 8)
    }

    /// Its speed byte across (+0x10).
    pub fn speed(&self) -> i32 {
        self.vx
    }

    pub fn falling(&self) -> bool {
        self.vy != 0
    }

    /// One frame: move, then the player contact (0x0806B9DC). False once it is gone.
    pub fn step(&mut self, player: &mut Player, camera: (i32, i32), level_h: i32) -> (bool, Contact) {
        self.vy = self.vy.min(MAX_FALL);
        self.y += self.vy * 16;
        let old = self.x >> 8;
        self.x += self.vx * 16;
        let dx = (self.x >> 8) - old;
        let contact = self.contact(player, dx);
        if self.vy != 0 || contact.standing {
            if self.vy == 0 {
                self.vx = 0;
            }
            self.vy += 1;
        }
        let (x, y) = self.px();
        let alive = x <= camera.0 + OFFSCREEN_RIGHT && x >= camera.0 - OFFSCREEN_LEFT && y < level_h + TILE_PX;
        (alive, contact)
    }

    /// 0x0806B530 for type 0x36.
    fn contact(&self, p: &mut Player, dx: i32) -> Contact {
        let (lx, ly) = self.px();
        let (bdx, bw, bdy, bh) = BOX;
        let (px, py) = p.px();
        let overlap = lx + bdx < px + HITBOX_W && px < lx + bdx + bw && ly + bdy < py + p.height() && py < ly + bdy + bh;
        if !overlap {
            return Contact::default();
        }
        let fy = p.frame().1;
        if fy + LAND_ABOVE < ly {
            if p.vy < 0 {
                return Contact::default();
            }
            // 0x0806B4B4: on top, carried across (0x080A3C6C lands the player).
            let feet = ly - STAND_OFFSET + 32;
            p.y = (feet - p.height()) * SUB;
            p.vy = 0;
            p.on_ground = true;
            p.x += dx * SUB;
            return Contact { standing: true, head: false };
        }
        let below = if p.form == Form::Small || p.crouch { SIDE_BELOW_SMALL } else { 0 };
        if fy + below < ly + SIDE_BELOW {
            // Side: against a lift that does not move across, the push stops; in front of
            // a moving one the player goes at its speed.
            let right_of = px + HITBOX_W / 2 > lx + bdx + bw / 2;
            if self.vx == 0 {
                if (p.vx > 0 && !right_of) || (p.vx < 0 && right_of) {
                    p.vx = 0;
                }
            } else if right_of == (self.vx > 0) {
                p.vx = self.vx * UNIT;
            }
            return Contact::default();
        }
        if p.vy < 0 {
            p.vy = HEAD_PUSH * UNIT;
            return Contact { standing: false, head: true };
        }
        Contact::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lift() -> Lift {
        Lift::spawn(&EnemySpawn { kind: LIFT, col: 4, row: 6, ..Default::default() })
    }

    #[test]
    fn it_drifts_left_until_stood_on_then_falls_ever_faster() {
        let mut l = lift();
        assert_eq!(l.px(), (64, 95), "1 px above its row");
        let mut away = Player::spawn(300, 0);
        for _ in 0..32 {
            l.step(&mut away, (0, 0), 1000);
        }
        assert_eq!(l.px(), (48, 95), "1/2 px a frame to the left");
        // The player lands on it: it stops across and starts to fall.
        let mut p = Player::spawn(l.px().0 + 10, l.px().1 - 16);
        p.vy = 16;
        let (_, c) = l.step(&mut p, (0, 0), 1000);
        assert!(c.standing && p.on_ground);
        assert_eq!(p.frame().1, l.px().1 - STAND_OFFSET);
        let x = l.px().0;
        let mut speeds = Vec::new();
        for _ in 0..0x50 {
            let y = l.y;
            l.step(&mut p, (0, 0), 1000);
            speeds.push(l.y - y);
        }
        assert_eq!(l.px().0, x, "no longer drifts");
        assert_eq!(speeds[0], 16, "speed 1 the frame after");
        assert_eq!(*speeds.last().unwrap(), MAX_FALL * 16, "capped at 4 px a frame");
    }
}
