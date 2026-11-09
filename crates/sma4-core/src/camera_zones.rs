//! The ROM's vertical scroll (the player loop 0x0809869C -> 0x080992A6) and the enemy-list
//! triggers that act on it (World 1-2's and 1-5's).
//!
//! * The camera y (0x03003754) follows the player's frame y (+4) with a dead zone: it moves
//!   only while the player is less than `dead_zone[0]` or more than `dead_zone[1]` px below
//!   it, by at most 0x08172D6F[|speed byte| >> 4] px a frame (0x080996F0). In scroll mode
//!   0 (header byte 6 bits 5-6, 0x03003CB2), once at 0xF0 or lower it does not go above
//!   0xF0 unless the player flies (0x08099624); mode 1 (World 1-5) follows up and down
//!   freely. Never above the level's top (+0xDA) nor below its bottom - 0x9F.
//! * Scroll mode 2 (the fortress, 1-1's bonus room, the Hammer Bro arenas, the Toad
//!   Houses; 0x08099A48) keeps the camera in a window 0x20 px tall from +0x9C, which is
//!   0xF0 if the camera is at 0xF0 or lower when it is set, else the level's top
//!   (0x080A86B0: at load, after a pipe's way out down, at Boom Boom's lock). It moves at
//!   most 3 px a frame up and 4 down, whatever the player's speed. At load (0x08014F20)
//!   the camera starts at 0x20 if the level's start camera is above 0xB0, else 0xF0 (0x110
//!   with the player 256 px or more down). The Toad Houses (0x102, 0x106) hold it at
//!   0x100.
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
/// Scroll mode 2 (0x08099A48): the window's height below +0x9C and its steps up / down.
const WINDOW: i32 = 0x20;
const WINDOW_UP: i32 = 3;
const WINDOW_DOWN: i32 = 4;
/// 0x08014F20, mode 2: a start camera below this starts at 0x20, else at 0xF0 (or 0x110
/// with the player's y at 0x100 or lower).
const WINDOW_START_HIGH: i32 = 0xB0;
const WINDOW_START_TOP: i32 = 0x20;
const WINDOW_START_LOW: i32 = 0x110;
/// 0x08099A48 holds the camera of the levels whose data is at 0x0815B574 (0x102, the
/// map's Toad Houses) and 0x0815BA83 (0x106, World 1-3's hidden one) at 0x100.
const HOUSES: [u16; 2] = [0x102, 0x106];
const HOUSE_CAMERA_Y: i32 = 0x100;

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
    /// Scroll mode 2's window top (player +0x9C, 0x080A86B0).
    base: i32,
    /// The level id (0x03003B0A): 0x08099A48 special-cases the Toad Houses.
    level: u16,
}

impl CameraZones {
    /// The triggers of an enemy list in level `level` with these settings, the camera
    /// starting at `y` (the header's, 0x0813600E) with the player at `player_y` (frame y).
    /// A band's left end pairs with the nearest right end after it with the same slot
    /// (0x0805780C).
    pub fn new(list: &[EnemySpawn], params: CameraParams, y: i32, level: u16, player_y: i32) -> Self {
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
        let mut z = Self { params, locks, bands, lock: None, y, last: y, base: 0, level };
        // 0x08014F20: +0x9C from the header's camera, then mode 2's own start (unless the
        // camera y is fixed, +0xC8 bit 15).
        z.rebase();
        if params.mode == 2 && params.fixed & 0x8000 == 0 {
            let y = if y < WINDOW_START_HIGH {
                WINDOW_START_TOP
            } else if player_y >= 0x100 {
                WINDOW_START_LOW
            } else {
                GROUND_TOP
            };
            z.set(y);
        }
        z
    }

    /// 0x080A86B0: in scroll mode 2, the window starts at 0xF0 if the camera is there or
    /// lower, else at the level's top (after a pipe's way out down or sideways, 0x080A8764
    /// / 0x080A8CC0, and when Boom Boom locks the camera, 0x0806CFEC).
    pub fn rebase(&mut self) {
        if self.params.mode == 2 {
            self.base = if self.y >= GROUND_TOP { GROUND_TOP } else { self.params.top as i32 };
        }
    }

    /// Puts the camera at `y` (a pipe's arrival).
    pub fn set(&mut self, y: i32) {
        self.y = y;
        self.last = y;
    }

    /// Puts the camera where following the player would leave it (debug starts): the
    /// player at the dead zone's bottom, within the level. In scroll mode 2 the window is
    /// the one (from the top or from 0xF0) that comes closest, as if he had come in by a
    /// pipe there.
    pub fn settle(&mut self, player: &Player) {
        let p = self.params;
        let lowest = p.bottom as i32 - VIEW_LAST_ROW;
        let mut y = (player.frame().1 - p.dead_zone[1] as i32).clamp(p.top as i32, lowest);
        if p.mode == 2 && !HOUSES.contains(&self.level) {
            self.base = if y > p.top as i32 + WINDOW { GROUND_TOP } else { p.top as i32 };
            y = y.clamp(self.base, (self.base + WINDOW).min(lowest).max(self.base));
        }
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
        if p.mode == 2 {
            self.window(player);
            return;
        }
        // 0x0809869C picks the flying dead zone while the player flies (+0xC6), and
        // 0x08099624 also below 0xF0, where the camera may rise to the level's top. Scroll
        // mode 1 (0x080992A6) skips 0x08099624: the normal dead zone (classic game,
        // 0x03003A2F clear) and the level's top, wherever the camera is.
        let flying = player.flight != 0;
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
        let y = self.band_limit(y);
        // TODO(re): 0x080A1A4C's scroll stops on tiles (none in World 1-5 checked).
        self.y = y;
        self.last = y;
    }

    /// 0x08099C60: the first armed band (by slot) keeps the camera above its limit.
    fn band_limit(&self, mut y: i32) -> i32 {
        if let Some(b) = (0..4).find_map(|slot| self.bands.iter().find(|b| b.slot == slot && b.armed)) {
            let limit = b.limit.max(self.params.top as i32);
            if limit < self.y {
                y = self.y + (limit - self.y).max(-TRIGGER_STEP);
            } else if y >= self.y && limit <= y {
                y = limit;
            }
        }
        y
    }

    /// Scroll mode 2 (0x080992A6 -> 0x08099A48): the classic dead zone (+0xD0 / +0xD4,
    /// 0x03003A2F clear) at a fixed 3 px a frame up and 4 down (3 up at once while the
    /// player flies upward, +0xC6), kept within the level and the window from +0x9C.
    fn window(&mut self, player: &Player) {
        let p = self.params;
        let zone = p.dead_zone.map(i32::from);
        let d = player.frame().1 - self.last;
        let delta = if player.flight != 0 && d < 0 {
            -WINDOW_UP
        } else if d < zone[0] {
            (d - zone[0]).max(-WINDOW_UP)
        } else if d < zone[1] {
            0
        } else {
            (d - zone[1]).min(WINDOW_DOWN)
        };
        let lowest = p.bottom as i32 - VIEW_LAST_ROW;
        let mut y = self.y + delta;
        if y >= lowest {
            y = lowest;
        } else if y < self.base {
            y = self.base;
        } else if y >= self.base + WINDOW {
            y = self.base + WINDOW;
        }
        let mut y = self.band_limit(y);
        // TODO(re): player +0x104 bit 0 leaves 0x03003754 alone and sets 0x03003CE0 to
        // 0x108; level 0x240 (Toad's spade room, 0x081628A0) holds 0x110.
        if HOUSES.contains(&self.level) {
            y = HOUSE_CAMERA_Y;
        }
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
            0x82,
            0,
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
        let mut z = CameraZones::new(&[], CameraParams { mode: 0, ..PARAMS }, 0, 0, 0);
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

    /// 1-1's bonus room (0x23) and the fortress (0x1CB): scroll mode 2.
    const BONUS_ROOM: CameraParams =
        CameraParams { bottom: 0x19F, fixed: 0, dead_zone: [0x20, 0x40], dead_zone_flying: [0x20, 0x40], top: 0, mode: 2 };
    const FORTRESS: CameraParams = CameraParams { dead_zone: [0x10, 0x30], dead_zone_flying: [0x10, 0x30], ..BONUS_ROOM };

    #[test]
    fn mode_2_starts_high_and_stays_in_a_window_0x20_tall_from_the_top() {
        // The header's camera 0x30 is above 0xB0: the camera starts at 0x20, the window at
        // the level's top.
        let mut z = CameraZones::new(&[], BONUS_ROOM, 0x30, 0x23, 0x150);
        assert_eq!(z.y, 0x20);
        // Far below: the camera does not leave the window.
        let p = big_at(10 * TILE_PX, 0x180);
        for _ in 0..50 {
            z.step(&p);
        }
        assert_eq!(z.y, 0x20);
        // Up high: 3 px a frame, down to the window's top.
        let p = big_at(10 * TILE_PX, 0x08);
        z.step(&p);
        assert_eq!(z.y, 0x1D);
        for _ in 0..50 {
            z.step(&p);
        }
        assert_eq!(z.y, 0);
    }

    #[test]
    fn mode_2_follows_down_4_px_a_frame_and_holds_the_toad_houses() {
        // The fortress: a low start camera puts the window at 0xF0; the level's bottom
        // (0x19F - 0x9F = 0x100) comes before the window's.
        let mut z = CameraZones::new(&[], FORTRESS, 0xF0, 0x1CB, 0xC0);
        assert_eq!(z.y, 0xF0);
        let p = big_at(10 * TILE_PX, 0x150);
        z.step(&p);
        assert_eq!(z.y, 0xF4, "0x150 - 0xF0 - 0x30 = 0x30, at most 4 down");
        for _ in 0..10 {
            z.step(&p);
        }
        assert_eq!(z.y, 0x100);
        let p = big_at(10 * TILE_PX, 0x20);
        for _ in 0..50 {
            z.step(&p);
        }
        assert_eq!(z.y, 0xF0, "not above the window");
        // The player 256 px or more down starts the camera at 0x110 (the level's bottom
        // pulls it back on the first frame).
        let z = CameraZones::new(&[], FORTRESS, 0xF0, 0x1CB, 0x100);
        assert_eq!(z.y, 0x110);
        // World 1-3's hidden Toad House.
        let mut z = CameraZones::new(&[], CameraParams { bottom: 0x1AF, ..FORTRESS }, 0xF0, 0x106, 0x110);
        z.step(&big_at(10 * TILE_PX, 0x130));
        assert_eq!(z.y, 0x100);
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
