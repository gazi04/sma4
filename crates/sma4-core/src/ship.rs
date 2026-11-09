//! The airship's deck (level 0x205): its cannons, cannonballs, the smoke they fire with,
//! the Bullet Bill blasters and the propellers under the hull. See docs/re-notes.md
//! "King's castle and the airship".
//!
//! * Generators (list types >= 0x190, kind = type - 0x18F; 0x0804B460 puts the newest in
//!   slot 0 of 0x03007848 + 0xA54, eight): 0x08090448 runs slots 7 down to 0, counting
//!   +0x0A and +0x0B down first, then the kind's handler 0x08170C80[kind - 1].
//! * Kinds 9-0x11 (0x08090338, the diagonal cannons): with +0x0A at 0 and the generator on
//!   screen (0 <= x - camera < 0xF0): +0x0A = 0x87 and a cannonball in direction kind + 1.
//! * Kind 4 (0x0808FF84, the rotating cannon): every 0x3D frames (+0x0B), when on screen
//!   (and not within 8 px of its left edge), +0x0A = 0x20 and +0x0C + 1; at +0x0A 0x1D the
//!   barrel turns (BG tile change 0x15), at 1 two balls, directions +0x0C & 7 and
//!   (+0x0C + 4) & 7.
//! * Kind 1 (0x0808FCF4, the Bullet Bill blaster): with +0x0A at 0 and on screen, +0x0A =
//!   (random & 0x1F) | 0x80; unless the player stands within 0x11 px of it, a Bullet Bill
//!   (object 0x78 in one of the slots 4..0, 0x0804CB98) comes out toward him.
//! * A cannonball (0x0808FEF0) is projectile 0x0B in the first free of slots 5..0: from
//!   the generator + (0x08170C20, 0x08170C36)[dir] at speeds (0x08170C4C, 0x08170C60)[dir];
//!   each one fired puts smoke at its mouth (0x0808FC30, effect 1, slots 5..0 of
//!   0x03007848 + 0x7E0, life 0x1F, screen coordinates).
//! * 0x0808C4EC runs the effects (0x08091F6C), then the projectiles (0x0808FBD0), then the
//!   generators, all after the objects: nothing moves in the frame it appears.

use sma4_assets::EnemySpawn;

use crate::enemies::{Enemy, PIPE_GOOMBA_SLOTS};
use crate::level::TILE_PX;
use crate::obj::ObjSprite;
use crate::player::{Player, SUB, UNIT};

/// Generator list types: the Bullet Bill blaster (kind 1), the rotating cannon (kind 4)
/// and the diagonal cannons (kinds 9-0x11).
pub const BLASTER: u16 = 0x190;
pub const ROTATING: u16 = 0x193;
pub const DIAGONAL_FIRST: u16 = 0x198;
pub const DIAGONAL_LAST: u16 = 0x1A0;
/// The propellers under the hull (list type 0xAA, init y + 3, main 0x0806F080).
pub const PROPELLER: u16 = 0xAA;
const PROPELLER_DY: i32 = 3;
/// Bullet Bill (object 0x78; 0x79, from kind 2, homes in and is not ported).
pub const BULLET_BILL: u16 = 0x78;

/// List types this module runs.
pub fn handles(kind: u16) -> bool {
    matches!(kind, BLASTER | ROTATING | PROPELLER | DIAGONAL_FIRST..=DIAGONAL_LAST)
}

/// Generator slots (0x08090448 runs eight).
const GENERATOR_SLOTS: usize = 8;
/// Projectile slots the cannonballs may take (0x0808FEF0 searches 5 down to 0).
const BALL_SLOTS: usize = 6;
/// Effect slots (0x080904FC searches 5 down to 0, else takes slot 0).
const SMOKE_SLOTS: usize = 6;

/// First +0x0A of a new generator (0x0804B460; 0 for kind 4).
const FIRST_WAIT: u8 = 0x60;
/// Kinds 9-0x11: frames between shots.
const DIAGONAL_EVERY: u8 = 0x87;
/// Kind 4: +0x0B cycle, the shot timer, and its barrel-turn / fire points.
const ROTATING_CYCLE: u8 = 0x3D;
const ROTATING_WAIT: u8 = 0x20;
const ROTATING_TURN: u8 = 0x1D;
/// The blaster: the Bullet Bill's speed byte by side (0x08170C74), its +0x23 (drawn at
/// priority 2 while it leaves the blaster), and the player's clearance (0x11 px).
const BILL_SPEED: [i32; 2] = [0x18, -0x18];
const BILL_EMERGE: u8 = 0x0C;
const BLASTER_NEAR: i32 = 0x11;

/// 0x08170C20 / 0x08170C36: where a shot leaves the generator (x, y) by direction; the
/// Bullet Bill's smoke uses directions 0x14 / 0x15.
const MOUTH_X: [i8; 22] = [8, 0x18, 0x1C, 0x18, 8, -8, -0xC, -8, -0xC, -0xC, -8, 8, -8, 8, 0, 0, 0, 0, 0xC, 0xC, 0xC, -0xC];
const MOUTH_Y: [i8; 22] = [-0xD, -9, 7, 0x17, 0x1B, 0x17, 7, -9, 0, 8, -8, -8, 8, 8, 0, 0, 0, 0, 0, 0, 0, 0];
/// 0x08170C4C / 0x08170C60: the ball's speeds (1/16 px a frame) by direction.
const BALL_VX: [i8; 20] = [0, 0xB, 0x10, 0xB, 0, -0xB, -0x10, -0xB, -0x10, -0x10, -0xB, 0xB, -0xB, 0xB, -0xB, 0xB, -0xB, 0xB, 0x10, 0x10];
const BALL_VY: [i8; 20] = [-0x10, -0xB, 0, 0xB, 0x10, 0xB, 0, -0xB, 0, 0, -0xB, -0xB, 0xB, 0xB, -0xB, -0xB, 0xB, 0xB, 0, 0];
const BILL_SMOKE_DIR: usize = 0x14;

/// Cannonball (0x0808E42C): gravity +2 up to 0x6E (0x0808D0E4), cancelled until it is
/// stomped; tile 0x106 pal 5 (0x0808E0A8).
const BALL_GRAVITY: i32 = 2;
const BALL_FALL_MAX: i32 = 0x6E;
const BALL_TILE: u16 = 0x106;
const BALL_PAL: u8 = 5;
/// 0x0808E0A8's player contact: (ball y - frame y - 0x08170B26[big]) as u16 below
/// 0x08170B28[big], (ball x - frame x + 8) as u16 below 0x14; a stomp unless the ball is
/// less than 0x13 px below the player's screen y.
const BALL_TOP: [i32; 2] = [0x10, 0];
const BALL_H: [i32; 2] = [0x10, 0x20];
const BALL_W: i32 = 0x14;
const BALL_STOMP: i32 = 0x13;
/// The stomp's bounce (+0x1D = 0xD0).
pub const BALL_BOUNCE: i32 = -0x30 * UNIT;

/// Smoke: life 0x1F, tiles 0x08170CD6[life >> 3], two 8x16 OBJs (pal 1).
const SMOKE_LIFE: u8 = 0x1F;
const SMOKE_TILES: [u16; 4] = [0x83, 0x82, 0x81, 0x80];
const SMOKE_PAL: u8 = 1;

/// Propeller: tiles 0x108 / 0x109 pal 6; y offsets 0x0816F4A8 / 0x0816F4AA by v-flip.
const PROPELLER_TILES: [u16; 2] = [0x108, 0x109];
const PROPELLER_PAL: u8 = 6;
const PROPELLER_DY1: [i32; 2] = [-0x10, 8];
const PROPELLER_DY2: [i32; 2] = [0, -8];

/// Off-screen removal of objects (0x0804A0C4): more than 0x120 px right of the camera or
/// 0x40 px left of it.
const OFFSCREEN_RIGHT: i32 = 0x120;
const OFFSCREEN_LEFT: i32 = 0x40;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gun {
    /// The list entry's cell, px.
    pub x: i32,
    pub y: i32,
    /// Generator kind (+0x08).
    pub kind: u8,
    /// +0x0A, +0x0B, +0x0C.
    pub wait: u8,
    pub cycle: u8,
    pub count: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cannonball {
    /// 1/256 px (+0 / +4).
    pub x: i32,
    pub y: i32,
    /// 1/16 px a frame (+0x0A / +0x08).
    pub vx: i32,
    pub vy: i32,
    /// Stomped (+0x14): it falls and no longer touches anything.
    pub stomped: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Smoke {
    /// Screen position, 1/256 px (+4 / +0).
    pub x: i32,
    pub y: i32,
    /// +0x11.
    pub life: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Propeller {
    pub x: i32,
    pub y: i32,
}

/// What the cannonballs did to the player this frame.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BallHits {
    pub hurt: bool,
    /// Stomped balls (their top-left, px): the player bounces and scores.
    pub stomped: Vec<(i32, i32)>,
}

/// The frame's view of the world the guns need.
#[derive(Debug, Clone, Copy)]
pub struct View {
    /// Camera x (0x03003750) and the objects' camera y (0x03003CE0).
    pub camera: (i32, i32),
    /// The autoscroll's y step this frame (0x03003876), px.
    pub scroll_dy: i32,
    /// The frame counter 0x03003CF6 (flips smoke and propellers).
    pub counter: u8,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ship {
    /// Newest first (slot 0).
    pub guns: Vec<Gun>,
    pub balls: [Option<Cannonball>; BALL_SLOTS],
    pub smoke: [Option<Smoke>; SMOKE_SLOTS],
    pub propellers: Vec<Propeller>,
    /// Sound 0x8A (a shot) was started this frame.
    pub fired: bool,
}

impl Ship {
    /// A list entry whose column came up (0x0804B768).
    pub fn add(&mut self, s: &EnemySpawn) {
        let (x, y) = (s.col as i32 * TILE_PX, s.row as i32 * TILE_PX);
        if s.kind == PROPELLER {
            self.propellers.push(Propeller { x, y: y + PROPELLER_DY });
            return;
        }
        let kind = (s.kind - 0x18F) as u8;
        let wait = if kind == 4 { 0 } else { FIRST_WAIT };
        self.guns.insert(0, Gun { x, y, kind, wait, cycle: 0, count: 0 });
        self.guns.truncate(GENERATOR_SLOTS);
    }

    /// 0x08091F6C: the smoke ages and goes; with no autoscroll it stays where it was in
    /// the level (0x0809054C).
    pub fn step_smoke(&mut self, view: &View, scroll_dx: i32, autoscroll: bool) {
        for slot in &mut self.smoke {
            let Some(s) = slot else { continue };
            if s.life & 0x7F == 0 {
                *slot = None;
                continue;
            }
            s.life -= 1;
            if !autoscroll {
                s.y -= view.scroll_dy * SUB;
            }
            let gone = if (s.y >> 8) as u8 >= 0xD0 {
                true
            } else {
                if !autoscroll {
                    s.x -= scroll_dx * SUB;
                }
                (s.x >> 8) as u8 >= 0xF0
            };
            if gone {
                *slot = None;
            }
        }
    }

    /// 0x0808FBD0: the cannonballs leave the screen, move, and touch the player.
    pub fn step_balls(&mut self, view: &View, autoscroll: bool, player: &Player, camera_y: i32) -> BallHits {
        let mut hits = BallHits::default();
        let (cx, cy) = view.camera;
        for slot in self.balls.iter_mut().rev() {
            let Some(b) = slot else { continue };
            // 0x0808FB08: gone once x - camera + 8 or y + 0x14 - camera leaves 0..0x100.
            let sx = (b.x >> 8) - cx + 8;
            let sy = (b.y >> 8) + 0x14 - cy;
            if !(0..0x100).contains(&(sx as i16 as i32)) || !(0..0x100).contains(&(sy as i16 as i32)) {
                *slot = None;
                continue;
            }
            // 0x0808D0E4: a straight line, gravity cancelled until stomped.
            b.x += b.vx * 16;
            b.y += b.vy * 16;
            if b.vy < BALL_FALL_MAX {
                b.vy += BALL_GRAVITY;
            }
            if autoscroll {
                b.y += view.scroll_dy * SUB;
            }
            if !b.stomped {
                b.vy -= BALL_GRAVITY;
            }
            // (Player +0x62, set while dying, is never set here: the level stops then.)
            if b.stomped {
                continue;
            }
            match ball_contact(b, player, camera_y) {
                Some(true) => hits.hurt = true,
                Some(false) => {
                    b.stomped = true;
                    b.vx = 0;
                    b.vy = 0;
                    hits.stomped.push((b.x >> 8, b.y >> 8));
                }
                None => {}
            }
        }
        hits
    }

    /// 0x08090448: the generators, oldest first. `rand` is 0x03003879 (one byte per
    /// slot); Bullet Bills go into `enemies`.
    pub fn step_guns(&mut self, view: &View, rand: &[u8; 18], player: &Player, enemies: &mut Vec<Enemy>) {
        self.fired = false;
        let (cx, cy) = view.camera;
        for i in (0..self.guns.len()).rev() {
            let g = &mut self.guns[i];
            g.wait = g.wait.saturating_sub(1);
            g.cycle = g.cycle.saturating_sub(1);
            let sx = g.x - cx;
            let on_screen = (0..0x100).contains(&(sx as i16 as i32));
            let on_rows = (0..0x100).contains(&((g.y - cy) as i16 as i32));
            let g = *g;
            match g.kind {
                9..=0x11 => {
                    if g.wait == 0 && on_screen && sx < 0xF0 {
                        self.guns[i].wait = DIAGONAL_EVERY;
                        self.fire(&g, g.kind as usize + 1, view);
                    }
                }
                4 => {
                    if g.cycle == 0 {
                        self.guns[i].cycle = ROTATING_CYCLE;
                        // Not within 8 px of the screen's left edge ((sx + 0x20) >= 0x28).
                        if !on_rows || !on_screen || sx + 0x20 < 0x28 {
                            continue;
                        }
                        self.guns[i].wait = ROTATING_WAIT;
                        self.guns[i].count = g.count.wrapping_add(1);
                    }
                    let g = self.guns[i];
                    // TODO(re): at +0x0A == 0x1D the barrel turns (0x03003CF5 = 0x15, a BG
                    // tile change at the generator); not drawn.
                    let _ = ROTATING_TURN;
                    if g.wait == 1 {
                        self.fire(&g, (g.count & 7) as usize, view);
                        self.fire(&g, (g.count.wrapping_add(4) & 7) as usize, view);
                    }
                }
                1 => {
                    if g.wait != 0 || !on_rows || !on_screen {
                        continue;
                    }
                    self.guns[i].wait = (rand[i] & 0x1F) | 0x80;
                    let near = ((player.frame().0 - g.x + BLASTER_NEAR) as u16) < 2 * BLASTER_NEAR as u16;
                    if (sx + 0x20) & 0xFF < 0x20 || near {
                        continue;
                    }
                    // 0x0804CB98: the highest free of slots 4..0.
                    let used = |s: u8| enemies.iter().any(|e| e.from_pipe && e.slot == s);
                    let Some(slot) = (0..PIPE_GOOMBA_SLOTS as u8).rev().find(|&s| !used(s)) else { continue };
                    let mut bill = bullet_bill(g.x, g.y - 1, player);
                    bill.slot = slot;
                    let side = (bill.vx < 0) as usize;
                    enemies.push(bill);
                    self.smoke_at(&g, BILL_SMOKE_DIR + side, view);
                }
                _ => {}
            }
        }
    }

    /// 0x0808FEF0: a ball in the first free slot from 5 down, and its smoke.
    fn fire(&mut self, g: &Gun, dir: usize, view: &View) {
        let Some(slot) = (0..BALL_SLOTS).rev().find(|&s| self.balls[s].is_none()) else { return };
        self.balls[slot] = Some(Cannonball {
            x: (g.x + MOUTH_X[dir] as i32) * SUB,
            y: (g.y + MOUTH_Y[dir] as i32) * SUB,
            vx: BALL_VX[dir] as i32,
            vy: BALL_VY[dir] as i32,
            stomped: false,
        });
        self.smoke_at(g, dir, view);
    }

    /// 0x0808FC30: smoke at the generator's mouth when it is near the screen's rows.
    fn smoke_at(&mut self, g: &Gun, dir: usize, view: &View) {
        let (cx, cy) = view.camera;
        if !(0..0x100).contains(&((g.y - cy + 0x30) as u16 as i32)) {
            return;
        }
        self.fired = true;
        let slot = (0..SMOKE_SLOTS).rev().find(|&s| self.smoke[s].is_none()).unwrap_or(0);
        self.smoke[slot] = Some(Smoke {
            x: ((g.x + MOUTH_X[dir] as i32 - cx) as i16 as i32) * SUB,
            y: ((g.y + MOUTH_Y[dir] as i32 - cy) as i16 as i32) * SUB,
            life: SMOKE_LIFE,
        });
    }

    /// Propellers leave with the camera (0x0804A0C4).
    pub fn step_propellers(&mut self, camera_x: i32) {
        self.propellers.retain(|p| p.x <= camera_x + OFFSCREEN_RIGHT && p.x >= camera_x - OFFSCREEN_LEFT);
    }

    /// The OAM entries: smoke (0x0809054C), cannonballs (0x0808E0A8), propellers
    /// (0x0806F080) and the Bullet Bills among `enemies` (0x0805440C). Taken before
    /// `step_guns`: what the generators make is first drawn the next frame.
    pub fn sprites(&self, view: &View, enemies: &[Enemy]) -> Vec<ObjSprite> {
        let (cx, cy) = view.camera;
        let mut out = Vec::new();
        for s in self.smoke.iter().rev().flatten() {
            let (x, y) = ((s.x >> 8) & 0x1FF, (s.y >> 8) as i16 as i32);
            if !(0..0xB1).contains(&(y + 0x10)) {
                continue;
            }
            let tile = SMOKE_TILES[((s.life & 0x7F) >> 3) as usize];
            let v = view.counter & 2 != 0;
            for (dx, hflip, vflip) in [(0, false, v), (8, true, !v)] {
                out.push(ObjSprite {
                    x: wrap_x(x + dx),
                    y,
                    tile,
                    shape: 2,
                    pal: SMOKE_PAL,
                    hflip,
                    vflip,
                    prio: 1,
                    ..Default::default()
                });
            }
        }
        for b in self.balls.iter().rev().flatten() {
            out.push(ObjSprite {
                x: (b.x >> 8) - cx,
                y: (b.y >> 8) - cy,
                tile: BALL_TILE,
                size: 1,
                pal: BALL_PAL,
                prio: 1,
                ..Default::default()
            });
        }
        for e in enemies.iter().filter(|e| e.kind == BULLET_BILL) {
            let (x, y) = e.px();
            let (sx, sy) = (x - cx, y - cy + 1);
            let dead = e.state == crate::enemies::EnemyState::Dead;
            let prio = if e.emerging != 0 { 2 } else { 1 };
            let right = e.facing_right;
            for (k, tile) in [0x10E, 0x10F].into_iter().enumerate() {
                let dx = if right { 8 - 8 * k as i32 } else { 8 * k as i32 };
                out.push(ObjSprite { x: sx + dx, y: sy, tile, shape: 2, hflip: right, vflip: dead, prio, ..Default::default() });
            }
        }
        for p in &self.propellers {
            let (sx, sy) = (p.x - cx, p.y - cy);
            let v = view.counter & 2 != 0;
            let spin = view.counter & 4 != 0;
            let piece = |x: i32, dy: i32, tile: u16, hflip: bool| ObjSprite {
                x,
                y: sy + dy,
                tile,
                shape: 2,
                pal: PROPELLER_PAL,
                hflip,
                vflip: v,
                prio: 1,
                ..Default::default()
            };
            out.push(piece(sx + if spin { 2 } else { 0 }, PROPELLER_DY1[v as usize], PROPELLER_TILES[0], spin));
            out.push(piece(sx, PROPELLER_DY2[v as usize], PROPELLER_TILES[1], false));
        }
        // Off the screen's rows: the GBA would not show them.
        out.retain(|s| (-0x20..0xA0).contains(&s.y) && (-0x20..0xF0).contains(&s.x));
        out
    }
}

/// The 9-bit OAM x as a screen position.
fn wrap_x(x: i32) -> i32 {
    let x = x & 0x1FF;
    if x >= 0x100 { x - 0x200 } else { x }
}

/// 0x0808E0A8's player test: Some(true) hurts, Some(false) is a stomp (also any touch
/// with the star, +0x3D).
fn ball_contact(b: &Cannonball, player: &Player, camera_y: i32) -> Option<bool> {
    let (fx, fy) = player.frame();
    let big = (!player.small_box()) as usize;
    let (bx, by) = (b.x >> 8, b.y >> 8);
    let dy = (by - fy - BALL_TOP[big]) as u16;
    let dx = (bx - fx + 8) as u16;
    if dy >= BALL_H[big] as u16 || dx >= BALL_W as u16 {
        return None;
    }
    let screen_y = fy - camera_y;
    Some(player.star == 0 && by - camera_y - BALL_STOMP < screen_y)
}

/// A Bullet Bill out of a blaster at (x, y) px (0x0808FCF4): toward the player at speed
/// 0x18, drawn behind for +0x23 = 0x0C frames; it takes one of the five object slots
/// the Goomba pipes use and goes once off the screen.
pub fn bullet_bill(x: i32, y: i32, player: &Player) -> Enemy {
    let mut e = Enemy::spawn_at(BULLET_BILL, x, y, player);
    let left = player.frame().0 * SUB + player.x.rem_euclid(SUB) < x * SUB;
    e.vx = BILL_SPEED[left as usize] * 16;
    e.vy = 0;
    e.facing_right = !left;
    e.emerging = BILL_EMERGE;
    e.from_pipe = true;
    e
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(camera: (i32, i32)) -> View {
        View { camera, scroll_dy: 0, counter: 0 }
    }

    fn spawn(kind: u16, col: u16, row: u8) -> EnemySpawn {
        EnemySpawn { kind, col, row, extra: [0; 2] }
    }

    fn player_at(fx: i32, fy: i32) -> Player {
        let mut p = Player::default();
        p.set_frame_sub(fx * SUB, fy * SUB);
        p
    }

    /// Numbers from the mGBA recording of World 1's airship (first cannon, col 0x13).
    #[test]
    fn a_diagonal_cannon_fires_up_left_with_smoke_every_0x87_frames() {
        let mut ship = Ship::default();
        ship.add(&spawn(DIAGONAL_FIRST, 0x13, 0xD));
        let (player, mut enemies, rand) = (player_at(0x9D, 0xA0), Vec::new(), [0; 18]);
        let v = view((0x5F, 0x70));
        let mut shots = Vec::new();
        for f in 0..0x1F0 {
            ship.step_smoke(&v, 0, true);
            ship.step_balls(&v, true, &player, 0x70);
            ship.step_guns(&v, &rand, &player, &mut enemies);
            if ship.fired {
                shots.push(f);
            }
        }
        assert_eq!(shots, [0x5F, 0x5F + 0x87, 0x5F + 2 * 0x87]);
        let mut ship = Ship::default();
        ship.add(&spawn(DIAGONAL_FIRST, 0x13, 0xD));
        for _ in 0..0x60 {
            ship.step_guns(&v, &rand, &player, &mut enemies);
        }
        assert_eq!(ship.balls[5], Some(Cannonball { x: 0x12800, y: 0xC800, vx: -0xB, vy: -0xB, stomped: false }));
        assert_eq!(ship.smoke[5], Some(Smoke { x: 0xC900, y: 0x5800, life: 0x1F }));
        // One frame on: straight up-left, gravity cancelled.
        ship.step_balls(&v, true, &player, 0x70);
        assert_eq!(ship.balls[5].map(|b| (b.x, b.y, b.vy)), Some((0x12750, 0xC750, -0xB)));
    }

    #[test]
    fn a_cannon_off_the_screen_holds_its_fire() {
        let mut ship = Ship::default();
        ship.add(&spawn(DIAGONAL_FIRST, 0x13, 0xD));
        let (player, mut enemies) = (player_at(0, 0xA0), Vec::new());
        for _ in 0..0x100 {
            ship.step_guns(&view((0x30, 0x70)), &[0; 18], &player, &mut enemies);
        }
        assert!(ship.balls.iter().all(|b| b.is_none()));
    }

    #[test]
    fn the_blaster_sends_a_bullet_bill_toward_the_player_unless_he_is_next_to_it() {
        for (fx, fired, vx) in [(0xD0, true, -0x18 * 16), (0x1F2, true, 0x18 * 16), (0x1F0, false, 0), (0x1E8, false, 0)] {
            let mut ship = Ship::default();
            ship.add(&spawn(BLASTER, 0x1E, 0xC));
            let (player, mut enemies) = (player_at(fx, 0xA0), Vec::new());
            for _ in 0..0x60 {
                ship.step_guns(&view((0x110, 0x70)), &[0; 18], &player, &mut enemies);
            }
            assert_eq!(enemies.len(), fired as usize, "player at {fx:#x}");
            if let Some(b) = enemies.first() {
                assert_eq!((b.kind, b.x, b.y, b.vx, b.emerging, b.slot), (BULLET_BILL, 0x1E000, 0xBF00, vx, BILL_EMERGE, 4));
            }
            // Next try (random & 0x1F) | 0x80 frames later.
            assert_eq!(ship.guns[0].wait, 0x80);
        }
    }

    #[test]
    fn landing_on_a_cannonball_stops_it_and_touching_it_from_the_side_hurts() {
        // mGBA: the ball at (0x123, 0xC3) after the frame's move; the player's frame at
        // y 0xA6 lands on it, at 0xB1 is hit.
        for (fy, stomp) in [(0xA6, true), (0xB1, false)] {
            let mut ship = Ship::default();
            ship.balls[5] = Some(Cannonball { x: 0x12490, y: 0xC490, vx: -0xB, vy: -0xB, stomped: false });
            let hits = ship.step_balls(&view((0x62, 0x70)), true, &player_at(0x118, fy), 0x70);
            assert_eq!(hits.hurt, !stomp);
            assert_eq!(hits.stomped.len(), stomp as usize);
            let b = ship.balls[5].unwrap();
            assert_eq!((b.stomped, b.vx, b.vy), if stomp { (true, 0, 0) } else { (false, -0xB, -0xB) });
        }
    }

    #[test]
    fn the_rotating_cannon_fires_two_opposite_balls() {
        let mut ship = Ship::default();
        ship.add(&spawn(ROTATING, 0x38, 0xA));
        let (player, mut enemies) = (player_at(0x300, 0xA0), Vec::new());
        let v = view((0x300, 0x70));
        for _ in 0..0x20 {
            ship.step_guns(&v, &[0; 18], &player, &mut enemies);
        }
        // +0x0C = 1: directions 1 and 5.
        let balls: Vec<_> = ship.balls.iter().flatten().map(|b| (b.vx, b.vy)).collect();
        assert_eq!(balls, [(-0xB, 0xB), (0xB, -0xB)]);
    }
}
