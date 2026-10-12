//! Player movement on a metatile collision grid.
//!
//! Positions and velocities are integers in 1/256 pixel units, stepped once per frame.
//! The tuning constants are placeholders chosen for an SMB3-like feel; they will be
//! replaced by the values reverse engineered from the player routine.

use sma4_assets::TileKind;

use crate::Buttons;

/// Fixed-point scale: 1 pixel = 256 units.
pub const SUB: i32 = 256;
pub const TILE_PX: i32 = 16;

// TODO(re): replace with the ROM's player physics constants.
const WALK_MAX: i32 = 384; // 1.5 px/frame
const RUN_MAX: i32 = 640; // 2.5 px/frame
const ACCEL: i32 = 16;
const SKID_DECEL: i32 = 40;
const FRICTION: i32 = 16;
const AIR_ACCEL: i32 = 12;
const JUMP_SPEED: i32 = 1024; // 4 px/frame upward
const RUN_JUMP_BONUS: i32 = 128;
const GRAVITY_HOLD: i32 = 32; // rising with A held: long jump
const GRAVITY: i32 = 128;
const MAX_FALL: i32 = 1024;

/// Collision box size in pixels.
pub const HITBOX_W: i32 = 12;
pub const HITBOX_H: i32 = 16;

/// Tile collision map, row-major, `(0, 0)` = top-left metatile.
#[derive(Debug, Clone, Default)]
pub struct Collision {
    pub width: usize,
    pub height: usize,
    pub kinds: Vec<TileKind>,
}

impl Collision {
    /// Outside the map: walls on the left/right edges, open above and below.
    pub fn at(&self, tx: i32, ty: i32) -> TileKind {
        if tx < 0 || tx >= self.width as i32 {
            return TileKind::Solid;
        }
        if ty < 0 || ty >= self.height as i32 {
            return TileKind::Empty;
        }
        self.kinds[ty as usize * self.width + tx as usize]
    }

    pub fn pixel_height(&self) -> i32 {
        self.height as i32 * TILE_PX
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Player {
    /// Top-left of the hitbox, 1/256 px.
    pub x: i32,
    pub y: i32,
    pub vx: i32,
    pub vy: i32,
    pub on_ground: bool,
    pub facing_left: bool,
    /// Frames spent moving on the ground; drives the walk animation.
    pub walk_frames: u32,
}

impl Player {
    pub fn spawn(px: i32, py: i32) -> Self {
        Self { x: px * SUB, y: py * SUB, ..Self::default() }
    }

    pub fn px(&self) -> (i32, i32) {
        (self.x.div_euclid(SUB), self.y.div_euclid(SUB))
    }

    pub fn step(&mut self, held: Buttons, pressed: Buttons, map: &Collision) {
        self.horizontal(held);
        self.vertical(held, pressed);
        self.move_x(map);
        self.move_y(map);
        if self.on_ground && self.vx != 0 {
            self.walk_frames = self.walk_frames.wrapping_add(1);
        } else if self.on_ground {
            self.walk_frames = 0;
        }
    }

    fn horizontal(&mut self, held: Buttons) {
        let dir = match (held.contains(Buttons::LEFT), held.contains(Buttons::RIGHT)) {
            (true, false) => -1,
            (false, true) => 1,
            _ => 0,
        };
        let max = if held.contains(Buttons::B) { RUN_MAX } else { WALK_MAX };
        if dir != 0 {
            self.facing_left = dir < 0;
            let accel = if !self.on_ground {
                AIR_ACCEL
            } else if self.vx * dir < 0 {
                SKID_DECEL
            } else {
                ACCEL
            };
            self.vx += accel * dir;
            if self.vx * dir > max {
                // Over the cap (e.g. B released): bleed off speed instead of snapping.
                self.vx = (self.vx * dir - FRICTION).max(max) * dir;
            }
        } else if self.on_ground {
            self.vx = approach_zero(self.vx, FRICTION);
        }
    }

    fn vertical(&mut self, held: Buttons, pressed: Buttons) {
        if self.on_ground && pressed.contains(Buttons::A) {
            let bonus = if self.vx.abs() > WALK_MAX { RUN_JUMP_BONUS } else { 0 };
            self.vy = -(JUMP_SPEED + bonus);
            self.on_ground = false;
        }
        let g = if self.vy < 0 && held.contains(Buttons::A) { GRAVITY_HOLD } else { GRAVITY };
        self.vy = (self.vy + g).min(MAX_FALL);
    }

    fn move_x(&mut self, map: &Collision) {
        self.x += self.vx;
        let (px, py) = self.px();
        let rows = tile_span(py, HITBOX_H);
        if self.vx > 0 {
            let tx = (px + HITBOX_W - 1).div_euclid(TILE_PX);
            if rows.clone().any(|ty| map.at(tx, ty) == TileKind::Solid) {
                self.x = (tx * TILE_PX - HITBOX_W) * SUB;
                self.vx = 0;
            }
        } else if self.vx < 0 {
            let tx = px.div_euclid(TILE_PX);
            if rows.clone().any(|ty| map.at(tx, ty) == TileKind::Solid) {
                self.x = (tx + 1) * TILE_PX * SUB;
                self.vx = 0;
            }
        }
    }

    fn move_y(&mut self, map: &Collision) {
        let old_bottom = self.y.div_euclid(SUB) + HITBOX_H;
        self.y += self.vy;
        let (px, py) = self.px();
        let cols = tile_span(px, HITBOX_W);
        self.on_ground = false;
        if self.vy >= 0 {
            let ty = (py + HITBOX_H).div_euclid(TILE_PX);
            let top = ty * TILE_PX;
            // "Top" tiles only catch the player if they were above the tile last frame.
            let lands = cols.clone().any(|tx| match map.at(tx, ty) {
                TileKind::Solid => true,
                TileKind::Top => old_bottom <= top,
                TileKind::Empty => false,
            });
            if lands && py + HITBOX_H >= top {
                self.y = (top - HITBOX_H) * SUB;
                self.vy = 0;
                self.on_ground = true;
            }
        } else {
            let ty = py.div_euclid(TILE_PX);
            if cols.clone().any(|tx| map.at(tx, ty) == TileKind::Solid) {
                self.y = (ty + 1) * TILE_PX * SUB;
                self.vy = 0;
            }
        }
    }
}

/// Tile indices covered by `[start, start + len)` pixels.
fn tile_span(start: i32, len: i32) -> std::ops::RangeInclusive<i32> {
    start.div_euclid(TILE_PX)..=(start + len - 1).div_euclid(TILE_PX)
}

fn approach_zero(v: i32, step: i32) -> i32 {
    if v > 0 { (v - step).max(0) } else { (v + step).min(0) }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 10x6 map with a solid floor on the last row.
    fn floor() -> Collision {
        let (w, h) = (10, 6);
        let mut kinds = vec![TileKind::Empty; w * h];
        for x in 0..w {
            kinds[(h - 1) * w + x] = TileKind::Solid;
        }
        Collision { width: w, height: h, kinds }
    }

    fn settle(p: &mut Player, map: &Collision) {
        for _ in 0..120 {
            p.step(Buttons::empty(), Buttons::empty(), map);
        }
    }

    #[test]
    fn falls_and_lands_on_floor() {
        let map = floor();
        let mut p = Player::spawn(32, 0);
        settle(&mut p, &map);
        assert!(p.on_ground);
        assert_eq!(p.px().1, 5 * TILE_PX - HITBOX_H);
    }

    #[test]
    fn walks_right_up_to_walk_speed() {
        let map = floor();
        let mut p = Player::spawn(32, 0);
        settle(&mut p, &map);
        for _ in 0..60 {
            p.step(Buttons::RIGHT, Buttons::empty(), &map);
        }
        assert_eq!(p.vx, WALK_MAX);
    }

    #[test]
    fn jump_leaves_ground_and_returns() {
        let map = floor();
        let mut p = Player::spawn(32, 0);
        settle(&mut p, &map);
        let ground_y = p.y;
        p.step(Buttons::A, Buttons::A, &map);
        assert!(!p.on_ground && p.y < ground_y);
        settle(&mut p, &map);
        assert_eq!(p.y, ground_y);
    }

    #[test]
    fn wall_stops_player() {
        let map = floor();
        let mut p = Player::spawn(32, 0);
        settle(&mut p, &map);
        for _ in 0..300 {
            p.step(Buttons::LEFT, Buttons::empty(), &map);
        }
        assert_eq!(p.px().0, 0);
        assert_eq!(p.vx, 0);
    }

    #[test]
    fn top_tile_is_passable_from_below() {
        let mut map = floor();
        // Platform two rows above the floor.
        for x in 0..10 {
            map.kinds[3 * map.width + x] = TileKind::Top;
        }
        let mut p = Player::spawn(32, 4 * TILE_PX - HITBOX_H);
        settle(&mut p, &map);
        p.step(Buttons::A, Buttons::A, &map);
        for _ in 0..20 {
            p.step(Buttons::A, Buttons::empty(), &map);
        }
        // Jumped through the platform from below and then lands on it.
        settle(&mut p, &map);
        assert_eq!(p.px().1, 3 * TILE_PX - HITBOX_H);
    }
}
