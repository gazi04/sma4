//! Boom Boom, the mini-fortress boss (object type 0x4B; init 0x0806CB5C, main 0x0806DC24),
//! and the ball he leaves (type 0x4A, main 0x0806DF60). See docs/re-notes.md, "Boom Boom".
//!
//! * Phases +0x3A (0x0816F3A0): 0 waits for the camera to reach his arena, locks it and
//!   freezes the player; 1 runs the intro (0x0816F390: blocks close the arena behind the
//!   player one by one, then the camera slides 16 px right) and starts the fight; 2 runs
//!   back and forth toward the player, 3 also crouches and leaps, 4 runs faster. Each stomp
//!   moves him to the next phase (at most 4).
//! * A stomp (0x0806C84C): player speed 0xD0, points popup (phase + 7), one hit point less
//!   (+0x1E, 3); he is dazed for 0x30 frames (+0x24, harmless and untouchable), then gets
//!   up (+0x3D = 0x2F). Five fireballs (+0x36 from 0x25 down to 0x20) beat him too.
//! * Beaten (phase 5) he lies there for 0x80 frames (0x50 after fireballs), then bursts
//!   (0x0806C9FC: eight sparks, sound 0x5A, the clock stopped by 0x03003D0F) into the ball,
//!   which leaps up, falls and bounces. The star bursts him at once (star-chain points).
//!   Touching it (0x0806DDD4): course clear music 0x21, the clock is counted into the score
//!   (0x08066F08 with 100 / 10 points per step), then 0x40 frames later the level ends. The
//!   player keeps moving meanwhile (+0x105 bit 3 only mutes the P-meter sound).
//!
//! Positions in 1/256 px, speeds in 1/16 px per frame (the ROM's speed bytes).

use crate::level::{Level, TILE_PX};
use crate::player::{Player, SUB};
use sma4_assets::TileKind;

pub const BOOM_BOOM: u16 = 0x4B;

/// A stomp bounces the player with speed byte 0xD0 (in the port's 1/256 px units).
pub const STOMP_BOUNCE: i32 = -0x30 * 16;

/// Init: three hit points (+0x1E), fireball counter +0x36 = 0x25 (beaten at 0x20).
const HIT_POINTS: u8 = 3;
const FIRE_COUNT: u8 = 0x25;
const FIRE_BEATEN: u8 = 0x20;
/// Frames lying beaten before the burst: after the last stomp (+0x23 = 0x80) or the last
/// fireball (0x50).
const BEATEN_STOMP: u8 = 0x80;
const BEATEN_FIRE: u8 = 0x50;
/// Dazed after a stomp (+0x24 = 0x30); getting up takes +0x3D = 0x2F frames (frames
/// 0x0816F256[+0x3D >> 3]), with a hop (speed 0xD3) at +0x3D = 0x10.
const DAZED: u8 = 0x30;
const GET_UP: u8 = 0x2F;
const GET_UP_FRAMES: [u8; 8] = [0x0A, 0x06, 0x07, 0x07, 0x07, 0x07, 0x06, 0x0A];
const GET_UP_HOP: i32 = -0x2D;
/// The run's leap (0x0806D5CC, every 256 frames of +0x1C): +0x3D = 0x3F, speed 0x0816F24E
/// = +/-0x20 toward the player; then as getting up.
const LEAP_TIME: u8 = 0x3F;
const LEAP_SPEED: i32 = 0x20;
/// Run speed toward the player (0x0816F250), re-aimed every 0x40 frames; phase 4 runs at
/// 0x18 (0x0816F252 + 2 in World 1) re-aimed every 0x20.
const RUN_SPEED: i32 = 0x10;
const FAST_SPEED: i32 = 0x18;
/// Phase 3's crouch (+0x35 = 0x30 in World 1) before the big leap (speed 0xA0).
const CROUCH: u8 = 0x30;
const CROUCH_AT: u32 = 0x61;
const BIG_LEAP: i32 = -0x60;
/// Turns at the screen's edges (0x0806CEE0): screen x < 8 moving left, > 0xCF moving right.
const EDGE_LEFT: i32 = 8;
const EDGE_RIGHT: i32 = 0xCF;
/// Probes for class 0xB8 (0x08169E78[0xB8..]): feet (32, 16), head (0, 16), walls (17, 4)
/// and (17, 28) as (dy, dx).
const FEET: (i32, i32) = (32, 16);
const HEAD: (i32, i32) = (0, 16);
const WALL_LEFT: (i32, i32) = (17, 4);
const WALL_RIGHT: (i32, i32) = (17, 28);
/// A wall pushes him out 1 px (0x0816F24B).
const GRAVITY: i32 = 3;
const MAX_FALL: i32 = 0x40;
/// Contact box 0x08169F44[0xD]: (dx 4, w 17, dy 10, h 19).
pub const BOX: (i32, i32, i32, i32) = (4, 17, 10, 19);
/// Frames +0x17: 0-5 running, 7 waiting for the fight (touching him then hurts), 8 / 9
/// dazed (alternating every 4 frames), 10 / 6 / 7 getting up, 0xB crouching.
pub const FRAME_WAITING: u8 = 7;
pub const FRAME_BEATEN: u8 = 8;
const FRAME_CROUCH: u8 = 0x0B;

/// Arena 0 (the World 1 mini-fortress, +0x43 = 0): intro steps 0x0816F26E[0] = 0x90; a
/// block 0x0816F33E[0] = 0xE4 every 0x20 steps at y 0x100 + 0x0816F2A1[step >> 5].
const INTRO_STEPS: u8 = 0x90;
pub const INTRO_BLOCK: u16 = 0xE4;
const INTRO_BLOCK_Y: [i32; 6] = [0x00, 0x80, 0x70, 0x60, 0x50, 0xFF];
/// Then 0x10 frames still and the camera 1 px a frame for 0x10 frames (0x0806D424).
const SLIDE_WAIT: u8 = 0x10;
const SLIDE: u8 = 0x10;
/// Sparks of the burst (projectile 0x0D, 0x0806C98C): eight, from (x + 4, y + 16), speeds
/// 0x0816F1C0[i] across and [(i + 2) & 7] down, for 0x3F frames.
pub const SPARK_SPEEDS: [i32; 8] = [-32, -23, 0, 23, 32, 23, 0, -23];
pub const SPARK_LIFE: u8 = 0x3F;

/// The ball (type 0x4A): leaps with speed 0xC8 from 8 px right of Boom Boom.
const BALL_LEAP: i32 = -0x38;
/// Touched: the clock tally waits 0x150 frames (0x03003BB4), then 0x40 frames to leave.
pub const BALL_TALLY_DELAY: u16 = 0x150;
pub const BALL_LEAVE: u8 = 0x40;
/// Tally points per step (0x0816EE44[2..]): 100 per ten seconds, 10 per second (tens).
pub const BALL_TALLY_POINTS: [u32; 2] = [10, 1];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoomBoom {
    pub x: i32,
    pub y: i32,
    pub vx: i32,
    pub vy: i32,
    /// +0x13 probe bits: 1 / 2 walls, 4 ground, 8 ceiling.
    bits: u8,
    pub frame: u8,
    pub facing_right: bool,
    /// +0x1C, +0x3C counters; +0x3D getting-up / leap timer; +0x35 crouch timer.
    count: u8,
    walk: u8,
    anim: u8,
    crouch: u8,
    pub hp: u8,
    fire: u8,
    /// +0x23 timer, +0x24 dazed.
    timer: u8,
    pub dazed: u8,
    /// Fireball flash +0x25.
    pub flash: u8,
    pub phase: u8,
    /// Intro routine +0x3E and its counters +0x4E / +0x41.
    intro: u8,
    intro_count: u8,
    slide: u8,
}

/// What Boom Boom did this frame; the game applies it.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Events {
    /// Phase 0: the camera locks at this x (left limit; the player stops and is frozen).
    pub lock_camera: Option<i32>,
    /// Intro: a block appears at this cell (sound 0x57 and a puff 4 frames later).
    pub block: Option<(i32, i32)>,
    pub puff: Option<(i32, i32)>,
    /// Intro: the camera moves 1 px right.
    pub camera_step: bool,
    /// The fight starts: the camera stays where it is and the player moves again.
    pub fight: bool,
    /// Stomped: popup kind.
    pub stomped: Option<u8>,
    pub hurt_player: bool,
    /// Beaten: he bursts into the ball.
    pub burst: Option<Ball>,
}

impl BoomBoom {
    /// Init 0x0806CB5C at the list cell's top-left (px).
    pub fn spawn(x: i32, y: i32) -> Self {
        Self {
            x: x * SUB,
            y: y * SUB,
            vx: 0,
            vy: 0,
            bits: 0,
            frame: FRAME_WAITING,
            facing_right: false,
            count: 0,
            walk: 0,
            anim: 0,
            crouch: 0,
            hp: HIT_POINTS,
            fire: FIRE_COUNT,
            timer: 0,
            dazed: 0,
            flash: 0,
            phase: 0,
            intro: 0,
            intro_count: 0,
            slide: 0,
        }
    }

    pub fn px(&self) -> (i32, i32) {
        (self.x >> 8, self.y >> 8)
    }

    /// 0x08049F48 as 0 (the player is right of x) or 1.
    fn side(&self, player: &Player) -> usize {
        let frame_x = player.frame().0 * SUB + player.x.rem_euclid(SUB);
        if frame_x >= self.x { 0 } else { 1 }
    }

    fn toward(&self, player: &Player, speed: i32) -> i32 {
        if self.side(player) == 0 { speed } else { -speed }
    }

    /// Reverse (0x0804A028): speed and facing.
    fn turn(&mut self) {
        self.vx = -self.vx;
        self.facing_right = !self.facing_right;
    }

    fn solid(level: &Level, x: i32, y: i32) -> bool {
        level.at(x.div_euclid(TILE_PX), y.div_euclid(TILE_PX)) == TileKind::Solid
    }

    /// 0x08045ABC's probes after the move: walls, then the feet (falling) or the head.
    fn probe(&mut self, level: &Level, frame: u32) {
        let (px, py) = self.px();
        self.bits = 0;
        // Standing still, the probe and the bit alternate: even frames left (bit 2), odd
        // frames right (bit 1).
        let right = self.vx > 0 || (self.vx == 0 && frame & 1 == 1);
        let (dy, dx) = if right { WALL_RIGHT } else { WALL_LEFT };
        if Self::solid(level, px + dx, py + dy) {
            self.bits |= if right { 1 } else { 2 };
        }
        if self.vy < 0 {
            if Self::solid(level, px + HEAD.1, py + HEAD.0) {
                self.bits |= 8;
            }
        } else {
            let k = level.at((px + FEET.1).div_euclid(TILE_PX), (py + FEET.0).div_euclid(TILE_PX));
            if k != TileKind::Empty {
                self.bits |= 4;
            }
        }
    }

    /// 0x0806CF10: move (0x08049EC0), probe again, land / bump / turn. `screen_x` is +0xC,
    /// from this frame's draw (before the move).
    fn physics(&mut self, level: &Level, screen_x: i32, frame: u32) {
        self.x += self.vx * 16;
        self.y += self.vy * 16;
        self.probe(level, frame);
        self.vy = (self.vy + GRAVITY).min(MAX_FALL);
        self.probe(level, frame);
        let bits = self.bits;
        if bits & 4 != 0 {
            // 0x08045384: snap the feet to the tile top.
            self.y &= !0xFFF;
            self.vy = 0;
        }
        if bits & 8 != 0 {
            self.vy = 1;
        }
        if bits & 3 == 0 {
            // 0x0806CEE0: screen edges.
            let sx = screen_x;
            if (self.vx < 0 && sx < EDGE_LEFT) || (self.vx >= 0 && sx > EDGE_RIGHT) {
                self.turn();
            }
        } else {
            self.turn();
            self.x += [0, -1, 1, 0][(bits & 3) as usize] * SUB;
        }
    }

    /// 0x0806CFC0: the run cycle (frames 0-5, one step every 4 of +0x3C), then physics.
    fn run(&mut self, level: &Level, screen_x: i32, frame: u32) {
        if self.walk & 3 == 0 {
            self.frame += 1;
            if self.frame > 5 {
                self.frame = self.walk & 3;
            }
        }
        self.physics(level, screen_x, frame);
    }

    fn on_ground(&self) -> bool {
        self.bits & 4 != 0
    }

    /// One frame of main 0x0806DC24. `stomp` is the generic contact's verdict for this frame
    /// (0x080493AC: the player came down on him).
    pub fn step(
        &mut self,
        level: &Level,
        player: &Player,
        camera_x: i32,
        player_on_ground: bool,
        stomp: bool,
        frame: u32,
    ) -> Events {
        let mut ev = Events::default();
        // The object loop counts the timers down first; the draw sets +0xC.
        self.timer = self.timer.saturating_sub(1);
        self.dazed = self.dazed.saturating_sub(1);
        self.flash = self.flash.saturating_sub(1);
        let sx = self.px().0 - camera_x;
        if self.fire == FIRE_BEATEN {
            self.fire = FIRE_BEATEN - 1;
            self.phase = 5;
            self.timer = BEATEN_FIRE;
            return ev;
        }
        if self.phase == 5 {
            // 0x0806DB34.
            self.frame = FRAME_BEATEN;
            if self.timer == 0 {
                ev.burst = Some(Ball::burst(self));
            }
            return ev;
        }
        if self.dazed != 0 {
            if self.dazed == 1 {
                self.anim = GET_UP;
            }
            self.frame = if frame & 4 != 0 { 9 } else { 8 };
            return ev;
        }
        // 0x0806DB5C.
        if self.anim != 0 {
            self.physics(level, sx, frame);
            self.anim -= 1;
            self.frame = GET_UP_FRAMES[(self.anim >> 3) as usize];
            if self.anim == 0x10 {
                if self.on_ground() {
                    self.vy = GET_UP_HOP;
                    self.vx = self.toward(player, RUN_SPEED);
                } else {
                    self.anim = 0x11;
                }
            }
            self.ease(frame);
            self.contact(stomp, &mut ev);
            return ev;
        }
        self.contact(stomp, &mut ev);
        self.walk = self.walk.wrapping_add(1);
        match self.phase {
            0 => self.wait_for_camera(camera_x, player_on_ground, &mut ev),
            1 => self.intro_step(camera_x, &mut ev),
            2 => self.phase_run(level, player, sx, stomp, frame, &mut ev),
            3 => self.phase_crouch(level, player, sx, frame),
            4 => self.phase_fast(level, player, sx, frame),
            // Beaten by this frame's stomp: 0x0806DB34 runs at once.
            _ => self.frame = FRAME_BEATEN,
        }
        ev
    }

    /// Odd frames: the speed eases toward 0 by 1.
    fn ease(&mut self, frame: u32) {
        if frame & 1 != 0 && self.vx != 0 {
            self.vx += if self.vx > 0 { -1 } else { 1 };
        }
    }

    /// 0x0806C84C: the player landing on him.
    fn contact(&mut self, stomp: bool, ev: &mut Events) {
        if !stomp {
            return;
        }
        if self.frame == FRAME_WAITING {
            ev.hurt_player = true;
            return;
        }
        ev.stomped = Some(self.phase + 7);
        self.phase = (self.phase + 1).min(4);
        self.hp -= 1;
        if self.hp == 0 {
            self.timer = BEATEN_STOMP;
            self.phase = 5;
        } else {
            self.vy = 0;
            self.vx = 0;
            self.crouch = 0;
            self.walk = 0;
            self.dazed = DAZED;
        }
    }

    /// Phase 0 (0x0806CFEC, arena 0): the camera's low byte reaching 0xF4-0xFB snaps it to
    /// 0xF8 and locks its left side; once the player stands, the intro starts.
    fn wait_for_camera(&mut self, camera_x: i32, player_on_ground: bool, ev: &mut Events) {
        self.frame = FRAME_WAITING;
        if ((camera_x as u8).wrapping_add(0x0C)) > 7 {
            return;
        }
        let cam = (camera_x & !0xFF) | 0xF8;
        ev.lock_camera = Some(cam);
        if player_on_ground {
            self.phase = 1;
            self.intro = 0;
            self.slide = 0;
            self.intro_count = INTRO_STEPS;
        }
    }

    /// Phase 1 (0x0806D4F8 -> 0x0816F390[+0x3E]).
    fn intro_step(&mut self, camera_x: i32, ev: &mut Events) {
        match self.intro {
            0 => {
                // 0x0806D280.
                let step = (self.intro_count >> 5) as usize;
                if step == 0 {
                    self.intro = 1;
                    self.intro_count = SLIDE_WAIT;
                    self.slide = SLIDE;
                    return;
                }
                let at = (camera_x + 8, 0x100 + INTRO_BLOCK_Y[step]);
                match self.intro_count & 0x1F {
                    0 => ev.block = Some((at.0.div_euclid(TILE_PX), at.1.div_euclid(TILE_PX))),
                    4 => ev.puff = Some(at),
                    _ => {}
                }
                self.intro_count -= 1;
            }
            1 => {
                // 0x0806D424.
                if self.intro_count != 0 {
                    self.intro_count -= 1;
                } else if self.slide != 0 {
                    self.slide -= 1;
                    ev.camera_step = true;
                } else {
                    self.intro = 2;
                }
            }
            _ => {}
        }
        // 0x0806D4A0: done once the routine counter reaches 2.
        if self.intro >= 2 {
            ev.fight = true;
            self.anim = GET_UP;
            self.phase = 2;
        }
    }

    /// Phase 2 (0x0806D680): run toward the player, re-aimed every 0x40 frames; every 256
    /// frames a leap (0x0806D5CC: +0x3D = 0x3F at 0x20 toward the player).
    fn phase_run(&mut self, level: &Level, player: &Player, sx: i32, stomp: bool, frame: u32, ev: &mut Events) {
        self.count = self.count.wrapping_add(1);
        if self.count == 0 {
            self.anim = LEAP_TIME;
            self.vx = self.toward(player, LEAP_SPEED);
            self.anim -= 1;
            self.frame = GET_UP_FRAMES[(self.anim >> 3) as usize];
            self.ease(frame);
            self.contact(stomp, ev);
            self.physics(level, sx, frame);
            return;
        }
        if self.count & 0x3F == 0 {
            self.vx = self.toward(player, RUN_SPEED);
        }
        self.run(level, sx, frame);
    }

    /// Phase 3 (0x0806D8F4): run; once every 256 frames crouch 0x30 frames and leap high.
    fn phase_crouch(&mut self, level: &Level, player: &Player, sx: i32, frame: u32) {
        if self.crouch == 0 {
            // +0x13 from last frame.
            let grounded = self.on_ground();
            if !grounded {
                self.x += self.vx * 16;
            }
            self.run(level, sx, frame);
            if !grounded {
                return;
            }
            if frame & 0x3F == 0 {
                self.vx = self.toward(player, RUN_SPEED);
                return;
            }
            // ((frame & 0xFF) + 0x1F) & 0xFF7F == 0: the frame counter's low byte is 0x61.
            if frame & 0xFF == CROUCH_AT {
                self.crouch = CROUCH;
            }
            return;
        }
        if frame & 3 == 0 {
            if self.vx > 0 {
                self.vx -= 1;
            } else if self.vx < 0 {
                self.vx += 2;
            }
        }
        self.frame = FRAME_CROUCH;
        self.crouch -= 1;
        if self.crouch == 0 {
            self.vy = BIG_LEAP;
            self.vx = self.toward(player, RUN_SPEED);
        }
    }

    /// Phase 4 (0x0806DA98): +0x3C counts twice; run, re-aimed at 0x18 every 0x20 frames.
    fn phase_fast(&mut self, level: &Level, player: &Player, sx: i32, frame: u32) {
        self.walk = self.walk.wrapping_add(1);
        self.run(level, sx, frame);
        if self.on_ground() && frame & 0x1F == 0 {
            self.vx = self.toward(player, FAST_SPEED);
        }
    }

    /// The player has the star and touches him (0x08048960 -> 0x080488E4, then 0x0806C84C):
    /// phase 5 with the timer out, so 0x0806DB34 bursts him at once. Not while he waits for
    /// the fight (frame 7: +0x64 = 0x60D, star-proof) or is dazed (no contact then).
    pub fn star_hit(&mut self) -> Option<Ball> {
        if self.frame == FRAME_WAITING || self.dazed != 0 || self.phase == 5 {
            return None;
        }
        self.phase = 5;
        self.timer = 0;
        self.frame = FRAME_BEATEN;
        Some(Ball::burst(self))
    }

    /// A player fireball hit him (0x0808B2F0 with +0x36): sound 0x7F, he flashes 0x10
    /// frames.
    pub fn fireball(&mut self) {
        if self.fire > FIRE_BEATEN {
            self.fire -= 1;
            self.flash = 0x10;
        }
    }

    pub fn touches(&self, (x, y, w, h): (i32, i32, i32, i32)) -> bool {
        let (ex, ey) = self.px();
        let (dx, bw, dy, bh) = BOX;
        self.dazed == 0 && self.phase != 5 && ex + dx < x + w && x < ex + dx + bw && ey + dy < y + h && y < ey + dy + bh
    }
}

/// The ball (type 0x4A).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ball {
    pub x: i32,
    pub y: i32,
    vy: i32,
    /// Leaped up and started to fall (+0x35): can be touched.
    falling: bool,
    pub touched: bool,
    /// After the tally: +0x23 to leaving.
    pub leave_timer: u8,
}

impl Ball {
    fn burst(b: &BoomBoom) -> Self {
        Self { x: b.x + 8 * SUB, y: b.y, vy: BALL_LEAP, falling: false, touched: false, leave_timer: 0 }
    }

    pub fn px(&self) -> (i32, i32) {
        (self.x >> 8, self.y >> 8)
    }

    pub fn vy(&self) -> i32 {
        self.vy
    }

    /// 0x08069920: fall, land, bounce back up at a quarter of the speed while that is
    /// faster than 2. Returns true the frame the player touches it.
    pub fn step(&mut self, level: &Level, player_box: (i32, i32, i32, i32)) -> bool {
        if self.touched {
            return false;
        }
        if self.vy >= 0 {
            self.falling = true;
        }
        let (bx, by) = self.px();
        let (x, y, w, h) = player_box;
        // Touch box 0x08169F44[1] (as the goal card's).
        let touch = self.falling && bx + 1 < x + w && x < bx + 14 && by + 2 < y + h && y < by + 10;
        if touch {
            self.touched = true;
            return true;
        }
        // 0x08049EC0: move, probe (feet while not rising), gravity; then 0x08069920 lands
        // and bounces with a quarter of the speed after gravity.
        let rising = self.vy < 0;
        self.y += self.vy * 16;
        let (px, py) = self.px();
        let ground = !rising && level.at((px + 8).div_euclid(TILE_PX), (py + 16).div_euclid(TILE_PX)) != TileKind::Empty;
        self.vy = (self.vy + GRAVITY).min(MAX_FALL);
        if ground {
            let v = self.vy;
            self.y &= !0xFFF;
            self.vy = 0;
            let b = -(v >> 2);
            if b < -2 {
                self.vy = b;
            }
        }
        false
    }
}
