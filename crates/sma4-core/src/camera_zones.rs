//! The ROM's vertical scroll (the player loop 0x0809869C -> 0x080992A6) and the enemy-list
//! triggers that act on it (World 1-2's and 1-5's).
//!
//! * The camera y (0x03003754) follows the player's frame y (+4) with a dead zone: it moves
//!   only while the player is less than `dead_zone[0]` or more than `dead_zone[1]` px below
//!   it, by at most 0x08172D6F[|speed byte| >> 4] px a frame (0x080996F0). In scroll mode
//!   0 (header byte 6 bits 5-6, 0x03003CB2), once at 0xF0 or lower it does not go above
//!   0xF0 unless the player flies (0x08099624); mode 1 (World 1-5) follows up and down
//!   freely. Never above the level's top (+0xDA) nor below its bottom - 0x9F.
//! * 0xC8 (init 0x080589A8, main 0x08058A04): a box `w` x `h` tiles (extra byte 1, high /
//!   low nibble) from x - 8, rows down to its own; while the player's frame is in it, the
//!   camera heads for y - extra byte 2 (clamped to the level) at 3 px a frame instead
//!   (player +0xA2 / +0xAC, 0x0809C638).
//! * 0xE0 / 0xE1 (inits 0x0805780C / 0x08057AA4, mains 0x08057934 / 0x08057BE4): the left
//!   and right ends of a band with the same slot (extra byte 1). Once the player is inside
//!   it (seen the frame after entering, +0xA3[slot] = n | 0x80) with the frame's bottom at
//!   most 0x11 px under the marker's row (armed, +0xA3[slot] = n), the
//!   camera may not go below y - extra byte 2 (+0xAE[slot] - 0xA0) and climbs back at 3 px a
//!   frame (0x08099C60); leaving the band across ends it.
//!
//! The triggers are objects, updated after the player: the camera sees last frame's.

use sma4_assets::{CameraParams, EnemySpawn};

use crate::level::TILE_PX;
use crate::player::{Form, Player, UNIT};

pub const LOCK: u16 = 0xC8;
pub const BAND_LEFT: u16 = 0xE0;
pub const BAND_RIGHT: u16 = 0xE1;
/// 0x0809C638 / 0x08099C60: at most 3 px a frame toward a trigger's y.
const TRIGGER_STEP: i32 = 3;
/// The view is 0xA0 px tall: the lowest camera y is the level's last row - 0x9F.
const VIEW_LAST_ROW: i32 = 0x9F;
/// 0x08099624: below this the camera stays unless the player flies.
const GROUND_TOP: i32 = 0xF0;
/// 0x08172D6F: follow speed by |speed byte| >> 4.
const FOLLOW_SPEED: [i32; 9] = [2, 2, 3, 4, 5, 6, 7, 7, 32];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Lock {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    target: i32,
}

impl Lock {
    /// 0x08058A04's box test on the player's 16x32 frame (the small form's from 16 px
    /// lower).
    fn holds(&self, player: &Player) -> bool {
        let (fx, fy) = player.frame();
        let low = if player.form == Form::Small { fy + 0x10 } else { fy };
        (self.x - 8..self.x + self.w * TILE_PX - 8).contains(&fx)
            && self.y - (self.h - 1) * TILE_PX < fy + 0x20
            && low <= self.y + 0x10
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Band {
    slot: u8,
    left: i32,
    right: i32,
    y: i32,
    limit: i32,
    /// +0xA3[slot]: entered across (n | 0x80), then armed (n).
    seen: bool,
    armed: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CameraZones {
    params: CameraParams,
    locks: Vec<Lock>,
    bands: Vec<Band>,
    /// The lock holding the camera (+0xA2 / +0xAC), as of last frame.
    lock: Option<i32>,
    /// The camera y (0x03003754) and its value after last frame (0x03003CE0).
    pub y: i32,
    last: i32,
}

impl CameraZones {
    /// The triggers of an enemy list in a level with these settings, the camera starting
    /// at `y`. A band's left end pairs with the nearest right end after it with the same
    /// slot (0x0805780C).
    pub fn new(list: &[EnemySpawn], params: CameraParams, y: i32) -> Self {
        let lowest = params.bottom as i32 - VIEW_LAST_ROW;
        let at = |s: &EnemySpawn| (s.col as i32 * TILE_PX, s.row as i32 * TILE_PX);
        let locks = list
            .iter()
            .filter(|s| s.kind == LOCK)
            .map(|s| {
                let (x, y) = at(s);
                // 0x0804B5F4: +0x1C = extra byte 1 (size), +0x1D = extra byte 2; 0x080589A8
                // clamps the target to +0xDA .. +0xCA - 0x9F.
                let (w, h) = ((s.extra[0] >> 4) as i32, (s.extra[0] & 0xF) as i32);
                let target = y - s.extra[1] as i32;
                Lock { x, y, w, h, target: if target < params.top as i32 { params.top as i32 } else { target.min(lowest) } }
            })
            .collect();
        let bands = list
            .iter()
            .filter(|s| s.kind == BAND_LEFT)
            .filter_map(|l| {
                // 0x0804B5F4 for 0xE0 / 0xE1: +0x1D = extra byte 1 (slot), +0x1C = byte 2.
                let r = list
                    .iter()
                    .filter(|r| r.kind == BAND_RIGHT && r.col > l.col && r.extra[0] == l.extra[0])
                    .min_by_key(|r| r.col)?;
                let (x, y) = at(l);
                Some(Band {
                    slot: l.extra[0],
                    left: x - 8,
                    right: r.col as i32 * TILE_PX + 8,
                    y,
                    // +0xAE = y - byte 2 + 0xA0, at least 0xA0.
                    limit: (y - l.extra[1] as i32).max(0),
                    seen: false,
                    armed: false,
                })
            })
            .collect();
        Self { params, locks, bands, lock: None, y, last: y }
    }

    /// Puts the camera at `y` (a pipe's arrival).
    pub fn set(&mut self, y: i32) {
        self.y = y;
        self.last = y;
    }

    /// Puts the camera where following the player would leave it (debug starts): the
    /// player at the dead zone's bottom, within the level and below 0xF0 if it gets there.
    pub fn settle(&mut self, player: &Player) {
        let p = self.params;
        let lowest = p.bottom as i32 - VIEW_LAST_ROW;
        let y = (player.frame().1 - p.dead_zone[1] as i32).clamp(p.top as i32, lowest);
        self.set(y);
    }

    /// One frame of the scroll, after the player moved (0x080992A6), then the triggers
    /// (the objects' turn).
    pub fn step(&mut self, player: &Player) {
        self.scroll(player);
        self.lock = self.locks.iter().find(|l| l.holds(player)).map(|l| l.target);
        let (fx, fy) = player.frame();
        for b in &mut self.bands {
            if !(b.left..=b.right).contains(&fx) {
                (b.seen, b.armed) = (false, false);
            } else if !b.seen {
                b.seen = true;
            } else if !b.armed && fy + 0x1F <= b.y + 0x10 {
                b.armed = true;
            }
        }
    }

    fn scroll(&mut self, player: &Player) {
        let p = self.params;
        // +0xC8 bit 15: a fixed camera y.
        if p.fixed & 0x8000 != 0 {
            self.set((p.fixed & 0x7FFF) as i32);
            return;
        }
        // 0x0809C638: a lock moves the camera instead.
        if let Some(target) = self.lock {
            self.y += (target - self.y).clamp(-TRIGGER_STEP, TRIGGER_STEP);
            self.last = self.y;
            return;
        }
        // 0x0809869C picks the flying dead zone while the player flies (+0xC6), and
        // 0x08099624 also below 0xF0, where the camera may rise to the level's top. Scroll
        // mode 1 (0x080992A6) skips 0x08099624: the normal dead zone (classic game,
        // 0x03003A2F clear) and the level's top, wherever the camera is.
        let flying = player.flight != 0;
        // TODO(re): mode 2's windowed scroll (0x08099A48) still takes the mode-0 path.
        let (zone, top) = if p.mode == 1 {
            (p.dead_zone, p.top as i32)
        } else if self.y < GROUND_TOP || flying {
            (p.dead_zone_flying, p.top as i32)
        } else {
            (p.dead_zone, GROUND_TOP)
        };
        // 0x080996F0.
        let speed = FOLLOW_SPEED[((player.vy / UNIT).unsigned_abs() >> 4).min(8) as usize];
        let d = player.frame().1 - self.last;
        let delta = if flying && d < 0 {
            d.max(-speed)
        } else if d < zone[0] as i32 {
            (d - zone[0] as i32).max(-speed)
        } else if d < zone[1] as i32 {
            0
        } else {
            (d - zone[1] as i32).min(speed)
        };
        let lowest = p.bottom as i32 - VIEW_LAST_ROW;
        let mut y = self.y + delta;
        if y >= lowest {
            y = lowest;
        } else if y < top {
            y = top;
        }
        // 0x08099C60: the first armed band (by slot) keeps the camera above its limit.
        if let Some(b) = (0..4).find_map(|slot| self.bands.iter().find(|b| b.slot == slot && b.armed)) {
            let limit = b.limit.max(p.top as i32);
            if limit < self.y {
                y = self.y + (limit - self.y).max(-TRIGGER_STEP);
            } else if y >= self.y && limit <= y {
                y = limit;
            }
        }
        // TODO(re): 0x080A1A4C's scroll stops on tiles (none in World 1-5 checked).
        self.y = y;
        self.last = y;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PARAMS: CameraParams =
        CameraParams { bottom: 0x1AF, fixed: 0, dead_zone: [0x10, 0x30], dead_zone_flying: [0x10, 0x30], top: 7, mode: 1 };

    /// World 1-5's triggers: two locks on row 22 and a band from column 115 to 142.
    fn world_1_5(y: i32) -> CameraZones {
        let s = |kind, col, row, extra| EnemySpawn { kind, col, row, extra };
        CameraZones::new(
            &[
                s(LOCK, 69, 22, [0x85, 0x50]),
                s(LOCK, 77, 22, [0x75, 0x50]),
                s(BAND_LEFT, 115, 22, [0x00, 0x78]),
                s(BAND_RIGHT, 142, 22, [0x00, 0x78]),
            ],
            PARAMS,
            y,
        )
    }

    fn big_at(fx: i32, fy: i32) -> Player {
        let mut p = Player::spawn(0, 0);
        p.set_form(Form::Big);
        let (x, y) = p.frame();
        p.x += (fx - x) * crate::player::SUB;
        p.y += (fy - y) * crate::player::SUB;
        p
    }

    #[test]
    fn mode_0_follows_down_and_stays_at_0xf0_and_below() {
        let mut z = CameraZones::new(&[], CameraParams { mode: 0, ..PARAMS }, 0);
        let p = big_at(10 * TILE_PX, 0x120);
        for _ in 0..200 {
            z.step(&p);
        }
        assert_eq!(z.y, 0x120 - 0x30, "stops with the player at the dead zone's bottom");
        // Standing high up: the camera does not rise above 0xF0.
        let p = big_at(10 * TILE_PX, 0x80);
        for _ in 0..200 {
            z.step(&p);
        }
        assert_eq!(z.y, GROUND_TOP);
    }

    #[test]
    fn mode_1_follows_the_player_back_up_past_0xf0() {
        // World 1-5: down at the bottom, then up on the hills above the cave.
        let mut z = world_1_5(0);
        let p = big_at(10 * TILE_PX, 0x130);
        for _ in 0..200 {
            z.step(&p);
        }
        assert_eq!(z.y, 0x130 - 0x30, "below 0xF0");
        let p = big_at(10 * TILE_PX, 0x80);
        for _ in 0..200 {
            z.step(&p);
        }
        assert_eq!(z.y, 0x80 - 0x10, "stops with the player at the dead zone's top");
    }

    #[test]
    fn a_lock_holds_the_camera_at_the_bottom_over_its_box() {
        let mut z = world_1_5(0xF0);
        let p = big_at(70 * TILE_PX, 22 * TILE_PX - 32);
        z.step(&p);
        assert_eq!(z.y, 0xF2, "the first frame follows (speed 2); the lock is seen next");
        z.step(&p);
        assert_eq!(z.y, 0xF5, "3 px a frame toward 0x160 - 0x50 = 0x110");
        for _ in 0..20 {
            z.step(&p);
        }
        assert_eq!(z.y, 0x110);
    }

    #[test]
    fn the_band_keeps_the_camera_up_once_the_player_is_above_its_row() {
        let mut z = world_1_5(0x110);
        let p = big_at(120 * TILE_PX, 22 * TILE_PX - 32);
        // Seen, armed, then the camera sees it.
        z.step(&p);
        z.step(&p);
        z.step(&p);
        assert_eq!(z.y, 0x10D, "climbs to 0x160 - 0x78 = 0xE8 at 3 px a frame");
        for _ in 0..20 {
            z.step(&p);
        }
        assert_eq!(z.y, 0xE8);
    }
}
