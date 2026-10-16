//! Enemies, reimplemented from the ROM's object routines (see docs/re-notes.md,
//! "Walking enemy" and "Koopa Troopa").
//!
//! * Goomba 0x72, green Koopa Troopa 0x6C and green Paratroopa 0x6E share main 0x08053704:
//!   speed byte 0x0816E586 = +/-8 (1/16 px per frame), turn at walls, walk off ledges;
//!   the Paratroopa hops. Red Koopa Troopa 0x6D (main 0x080538FC) turns at ledges.
//! * Paragoomba 0x73 (main 0x08053E9C): walks like a Goomba, hops three times (speed byte
//!   0xF0), then high (0xD0) and rests 0x50 frames on the ground; stomped, it becomes a
//!   Goomba 0x72 that keeps its own palette.
//! * Init 0x080533A0: faces the player (direction table 0x0816E568, side test 0x08049F48).
//! * Contact 0x080493AC -> 0x08048F80: stomp if the player is high enough, else 0x08048E8C.
//! * Stomp 0x08048B3C: player bounces (0x0809D7C8). Goomba -> squished (state 7, 16
//!   frames); Koopa -> shell (state 3, timer 0xFF); Paratroopa -> Koopa (0x0816966C).
//! * Shell: touching it (or landing on it) kicks it (0x080436A0) to state 5, 3 px per frame
//!   away from the player; a kicked shell knocks out enemies it touches (0x08042C10).
//! * Piranha Plants 0xA0 (green) / 0xA2 (red) share main 0x08055C88: a 4-phase cycle
//!   (hidden, rising, out, sinking) on timer +0x23; they only hurt (+0x64 bit 0x1000) and
//!   vanish in a puff of smoke (state 8) when the tail or a shell hits them.
//! * The Venus Fire Trap 0xA4 (flags 0x0816E56E bit 7) uses the same main: it waits 0x60
//!   frames per phase, turns its head toward the player and, out of the pipe with its
//!   mouth shut, spits a fireball (projectile type 5, 0x08055938) at timer 0x28.

use sma4_assets::{
    EnemySpawn, KOOPA_SHELL, KOOPA_SHELL_WAKING, KOOPA_SPIN, PIRANHA_LOW, PUFF_FRAMES, TileKind, WALKER_SQUISHED,
};

use crate::level::{Level, TILE_PX};
use crate::player::{HITBOX_W, Player, SUB};

/// Goomba.
pub const WALKER: u16 = 0x72;
/// Green Koopa Troopa.
pub const KOOPA: u16 = 0x6C;
/// Red Koopa Troopa: turns around at ledges.
pub const RED_KOOPA: u16 = 0x6D;
/// Green Koopa Paratroopa: hops; loses its wings when stomped.
pub const PARATROOPA: u16 = 0x6E;
/// Paragoomba: hops and flutters; loses its wings when stomped.
pub const PARAGOOMBA: u16 = 0x73;
/// Green Piranha Plant: comes 0x19 px out of its pipe.
pub const PIRANHA: u16 = 0xA0;
/// Red Piranha Plant: comes 0x21 px out.
pub const RED_PIRANHA: u16 = 0xA2;
/// Green Venus Fire Trap (SMB3's 0xA4, flags 0x80): comes 0x19 px out and spits fire.
pub const FIRE_PIRANHA: u16 = 0xA4;
/// Types that live in a pipe and are listed at its top row (ceiling variants 0xA1 / 0xA3 /
/// 0xA5 / 0xA7 and the red Venus Fire Trap 0xA6 are not ported yet).
pub const PIRANHA_TYPES: [u16; 3] = [PIRANHA, RED_PIRANHA, FIRE_PIRANHA];
/// Enemy types implemented so far.
pub const SUPPORTED: [u16; 8] = [WALKER, KOOPA, RED_KOOPA, PARATROOPA, PARAGOOMBA, PIRANHA, RED_PIRANHA, FIRE_PIRANHA];

/// 8/16 px per frame.
const WALK_SPEED: i32 = 8 * 16;
// Shared object gravity (0x08049EC0): speed byte += 0x08169D9E[0] = 3 each frame, capped
// at 0x08169D9C[0] = 64 while falling (1/16 px per frame; water uses 1 and 16, not ported).
const GRAVITY: i32 = 3 * 16;
const MAX_FALL: i32 = 64 * 16;
/// Paratroopa (0x08053704, type 0x6E): on landing the speed byte becomes 0xD0, and each
/// frame in the air it is lowered by one more (gravity 2 instead of 3).
const HOP_SPEED: i32 = -48 * 16;
const HOP_LIFT: i32 = 16;
/// Paragoomba (0x08053E9C): on the ground with timer +0x23 at 0, it hops (speed byte
/// 0xF0) and counts the hop in +0x3B; the fourth is high (0xD0), resets the count and
/// sets the timer to 0x50. Gravity is the Paratroopa's (+3 - 1).
const PARA_HOP: i32 = -16 * 16;
const PARA_HIGH_HOP: i32 = -48 * 16;
const PARA_HOPS: u8 = 4;
const PARA_REST: u8 = 0x50;
/// On the ground it turns toward the player when its counter +0x3A is a multiple of 0x40.
const PARA_TURN_MASK: u32 = 0x3F;
const SIZE: i32 = 16;
/// Stomp check in 0x08048F80: the enemy counts as stomped when the player's 32 px frame is
/// at least 19 px above it, i.e. the player's feet are within its top 13 px.
const STOMP_DEPTH: i32 = 32 - 19;
/// Player speed after a stomp: byte 0xC0 = -64/16 px per frame.
pub const STOMP_BOUNCE: i32 = -64 * 16;
/// Squished state 7 lasts while timer +0x34 (set to 0x10) runs down.
const SQUISH_FRAMES: u8 = 0x10;
/// Shell timer +0x34 after a stomp (0x08048B30). The object loop 0x08044AF0 counts it down
/// every other frame while it is >= 0x60 and every frame below; at 0 the Koopa walks again.
const SHELL_TIMER: u8 = 0xFF;
const SHELL_SLOW_TICKS: u8 = 0x60;
/// Legs peek out of the shell while timer < 0x50 and timer & 0x08169436[timer >> 4] != 0.
const SHELL_LEGS_BELOW: u8 = 0x50;
const SHELL_LEG_MASKS: [u8; 5] = [2, 2, 4, 8, 0x10];
/// Kick speed byte 0x08169420[side] = -48 / +48 (away from the player), plus half the
/// player's speed when it points the same way.
const KICK_SPEED: i32 = 48;
/// Contact is ignored for 16 frames after a kick (+0x24).
const KICK_GRACE: u8 = 0x10;
/// ...and for 8 frames after a stomp (0x08048B3C sets +0x24 = 8; 0x080493AC skips the
/// contact test while it runs), so the bounce does not stomp a Paragoomba twice.
const STOMP_GRACE: u8 = 8;
/// Knocked out (0x08042BA4): state 6, speed byte 0xD0 up, +/-0x10 sideways.
const KNOCKOUT_SPEED: i32 = -48 * 16;
const KNOCKOUT_DRIFT: i32 = 16 * 16;
/// Tail hit (0x0804955C): sideways speed 0x08169D80 = -8/+8, contact ignored for 12
/// frames (+0x24 = 0x0C).
const TAIL_DRIFT: i32 = 8 * 16;
const TAIL_GRACE: u8 = 0x0C;
/// Piranha Plant init (0x0805510C / 0x08055134 -> 0x080550DC): x += 8 (centred on the
/// two-column pipe), home y (+0x35:+0x3A) = spawn y, reach +0x3F = 0x19 / 0x21 px.
const PLANT_X: i32 = 8;
const PLANT_REACH: i32 = 0x19;
const RED_PLANT_REACH: i32 = 0x21;
/// Each phase change (0x08055914) sets timer +0x23 to 0x30 (types below 0xA4; the
/// fire-spitting ones wait 0x60). The object loop counts it down every frame.
const PLANT_WAIT: u8 = 0x30;
const FIRE_PLANT_WAIT: u8 = 0x60;
/// Out of its pipe (0x08055B34), a fire plant spits when its timer is 0x28 (World 1, where
/// +0x3D | world is 0), or at 0x40 and 0x10 (later worlds).
const SPIT_AT: u8 = 0x28;
const SPIT_TWICE_AT: [u8; 2] = [0x40, 0x10];
/// Rising and sinking move 1 px per frame (speed byte -0x10 / 0x10, 0x08055AF0).
const PLANT_STEP: i32 = SUB;
/// A hidden plant stays in its pipe while the player is within 27 px horizontally
/// (0x08055A24: side test 0x08049F48 distance + 0x1B > 0x36 to come out).
const PLANT_SHY: u8 = 0x1B;
/// Draw 0x0805564C: while less than 0x11 px out, the stem is left off (frames +2).
const PLANT_LOW: i32 = 0x11;
/// Hit boxes 0x08169F44[+0x64 & 0x3F] (+0x64 = 0x1002 green / 0x100A red):
/// (dx, w, dy, h) = (2, 12, 2, 24) / (2, 12, 2, 20).
const PLANT_BOX: (i32, i32, i32) = (2, 12, 2);
const PLANT_BOX_H: i32 = 24;
const RED_PLANT_BOX_H: i32 = 20;
/// Puff of smoke (state 8, 0x08044550): timer +0x23 = 0x1F, frame = timer >> 3.
const PUFF_TIME: u8 = 0x1F;

/// Enemies spawn once they are this close to the player horizontally: half the GBA screen
/// plus a margin, so they appear just outside the view.
// TODO(re): use the ROM's spawn window (object loader keyed on the camera).
pub const SPAWN_DISTANCE: i32 = 120 + 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnemyState {
    Walking,
    /// Flattened after a stomp; removed when the timer reaches 0.
    Squished(u8),
    /// Koopa shell lying still (state 3), with its wake-up timer.
    Shell(u8),
    /// Kicked shell sliding along (state 5).
    Kicked,
    /// Knocked out by a shell: falls off the screen upside down (state 6).
    Dead,
    /// Piranha Plant: phase +0x3B & 3 (0 hidden in the pipe, 1 rising, 2 out, 3 sinking)
    /// and timer +0x23.
    Plant { phase: u8, timer: u8 },
    /// Puff of smoke left by a knocked-out Piranha Plant (state 8), with its timer.
    Puff(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Enemy {
    pub kind: u16,
    /// Top-left of the 16x16 box, 1/256 px.
    pub x: i32,
    pub y: i32,
    pub vx: i32,
    pub vy: i32,
    pub state: EnemyState,
    /// Frames alive; drives the walk animation.
    pub age: u32,
    /// Frames left in which player contact is ignored (after a kick).
    pub grace: u8,
    /// Red Koopa: already turned at this ledge (+0x3B).
    at_ledge: bool,
    /// Shell lying upside down after a tail hit (+0x1A bit 7).
    pub flipped: bool,
    /// Cell a sliding shell ran into this frame; the game bumps it like a head hit.
    pub wall_hit: Option<(i32, i32)>,
    /// Enemies this shell knocked out since it was kicked (+0x0E), for the score chain.
    pub knockouts: u16,
    /// Piranha Plant: y of the pipe top it hides below (+0x35:+0x3A), in px.
    pub home_y: i32,
    /// Piranha Plant: how far it comes out (+0x3F), in px.
    reach: i32,
    /// Piranha Plant: chomp counter +0x3C, +1 per frame.
    chomp: u8,
    /// Venus Fire Trap: looks right (+0x1A bit 6 from 0x0816E62C[side], main 0x08055C88)
    /// and up (the draw 0x0805564C v-flips the head while the player is above it).
    pub looks_right: bool,
    pub looks_up: bool,
    /// Venus Fire Trap: spits twice per appearance (0x08055B34: outside World 1).
    pub spits_twice: bool,
    /// Fireball spat this frame; the game puts it in a free projectile slot.
    pub spit: Option<Fireball>,
    /// Paragoomba: hops so far (+0x3B), the rest timer (+0x23) and the wing counter
    /// (+0x3C: +1 a frame, +3 while rising from the high hop, 0 on the ground before it).
    hops: u8,
    hop_timer: u8,
    wings: u8,
    /// Wing counter as this frame's draw saw it (0x08053D88 runs before the update).
    wings_drawn: u8,
    /// A Goomba that was a Paragoomba: it keeps the Paragoomba's palette (+0x21).
    pub was_para: bool,
}

/// What touching an enemy did this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Contact {
    None,
    /// Landed on it: the player bounces.
    Stomped,
    /// Kicked a shell from the side.
    Kicked,
    /// Landed on a resting shell, kicking it (0x08048E58: chain points, no bounce).
    KickedFromAbove,
    HurtPlayer,
}

/// Direction toward the player (0x08049F48 side test): +1 if the player is to the right.
fn toward(player: &Player, x_px: i32) -> i32 {
    if player.centre().0 >= x_px + SIZE / 2 { 1 } else { -1 }
}

impl Enemy {
    pub fn px(&self) -> (i32, i32) {
        (self.x.div_euclid(SUB), self.y.div_euclid(SUB))
    }

    fn spawn(s: &EnemySpawn, player: &Player) -> Self {
        Self::spawn_at(s.kind, s.col as i32 * TILE_PX, s.row as i32 * TILE_PX, player)
    }

    /// A walking enemy with its box's top-left at pixel `(x, y)`, facing the player.
    pub fn spawn_at(kind: u16, x: i32, y: i32, player: &Player) -> Self {
        if PIRANHA_TYPES.contains(&kind) {
            return Self::plant(kind, x, y);
        }
        Self {
            kind,
            x: x * SUB,
            y: y * SUB,
            vx: toward(player, x) * WALK_SPEED,
            vy: 0,
            state: EnemyState::Walking,
            age: 0,
            grace: 0,
            at_ledge: false,
            flipped: false,
            wall_hit: None,
            knockouts: 0,
            home_y: 0,
            reach: 0,
            chomp: 0,
            looks_right: false,
            looks_up: false,
            spits_twice: false,
            spit: None,
            hops: 0,
            hop_timer: 0,
            wings: 0,
            wings_drawn: 0,
            was_para: false,
        }
    }

    /// A Piranha Plant in the pipe whose top is at pixel `(x, y)` (left column, top row).
    fn plant(kind: u16, x: i32, y: i32) -> Self {
        Self {
            kind,
            x: (x + PLANT_X) * SUB,
            y: y * SUB,
            vx: 0,
            vy: 0,
            state: EnemyState::Plant { phase: 0, timer: 0 },
            age: 0,
            grace: 0,
            at_ledge: false,
            flipped: false,
            wall_hit: None,
            knockouts: 0,
            home_y: y,
            reach: if kind == RED_PIRANHA { RED_PLANT_REACH } else { PLANT_REACH },
            chomp: 0,
            looks_right: false,
            looks_up: false,
            spits_twice: false,
            spit: None,
            hops: 0,
            hop_timer: 0,
            wings: 0,
            wings_drawn: 0,
            was_para: false,
        }
    }

    pub fn is_plant(&self) -> bool {
        PIRANHA_TYPES.contains(&self.kind)
    }

    /// Piranha Plant drawn this frame: out of its pipe (main 0x08055C88 skips the draw in
    /// phase 0). Only the part above `home_y` shows: the ROM masks the rest with two OBJs
    /// (0x080A39EC) over the 32 px above the pipe top.
    pub fn plant_visible(&self) -> bool {
        matches!(self.state, EnemyState::Plant { phase: 1..=3, .. })
    }

    /// Spits fire (flags 0x0816E56E bit 7).
    pub fn is_fire_plant(&self) -> bool {
        self.kind == FIRE_PIRANHA
    }

    pub fn is_koopa(&self) -> bool {
        matches!(self.kind, KOOPA | RED_KOOPA | PARATROOPA)
    }

    /// Can hurt, be stomped or be knocked out.
    fn alive(&self) -> bool {
        !matches!(self.state, EnemyState::Squished(_) | EnemyState::Dead | EnemyState::Puff(_))
    }

    /// One frame. Returns false when the enemy should be removed.
    pub fn step(&mut self, level: &Level, player: &Player, frame: u32) -> bool {
        self.age = self.age.wrapping_add(1);
        self.grace = self.grace.saturating_sub(1);
        self.wall_hit = None;
        self.spit = None;
        match self.state {
            EnemyState::Squished(t) => {
                self.state = EnemyState::Squished(t - 1);
                return t > 1;
            }
            EnemyState::Walking if self.kind == RED_KOOPA => self.walk_red(level),
            EnemyState::Walking if self.kind == PARAGOOMBA => self.step_paragoomba(level, player),
            EnemyState::Walking => {
                if self.move_x(level) {
                    self.vx = -self.vx;
                }
                let landed = self.fall(level);
                if self.kind == PARATROOPA {
                    self.vy = if landed { HOP_SPEED } else { self.vy - HOP_LIFT };
                }
            }
            EnemyState::Shell(t) => self.shell(level, player, t, frame),
            EnemyState::Kicked => {
                // 0x08042FF0: the wall probe's cell is hit like a head hit (0x08042EF8 ->
                // 0x0809FC1C(4)), then the shell turns around.
                if self.move_x(level) {
                    self.wall_hit = Some(self.wall_probe());
                    self.vx = -self.vx;
                }
                self.fall(level);
            }
            EnemyState::Dead => {
                self.x += self.vx;
                self.vy = (self.vy + GRAVITY).min(MAX_FALL);
                self.y += self.vy;
            }
            EnemyState::Plant { phase, timer } => self.step_plant(player, phase, timer.saturating_sub(1)),
            EnemyState::Puff(t) => {
                self.state = EnemyState::Puff(t - 1);
                return t > 1;
            }
        }
        self.px().1 < level.pixel_height() + TILE_PX
    }

    /// Piranha Plant main 0x08055C88, phase handlers 0x0816E61C. `timer` has already been
    /// counted down for this frame (object loop 0x08044AF0).
    fn step_plant(&mut self, player: &Player, phase: u8, timer: u8) {
        let (x, y) = self.px();
        let (fx, fy) = player.frame();
        let fire = self.is_fire_plant();
        if fire && phase != 0 {
            // Main: face the player (side test 0x08049F48); the draw looks up while the
            // player is above (0x08049FDC).
            self.looks_right = fx - x >= 0;
            self.looks_up = fy - y < 0;
        }
        let advance = match phase {
            // 0x08055A24: once the timer runs out, come out unless the player is close.
            0 => {
                let d = (player.centre().0 - (self.px().0 + SIZE / 2)) as u8;
                timer == 0 && d.wrapping_add(PLANT_SHY) > 2 * PLANT_SHY
            }
            // 0x08055AFC: rise until `reach` px above home.
            1 if y > self.home_y - self.reach => {
                self.y -= PLANT_STEP;
                false
            }
            // 0x08055C04: sink until 1 px above home (it compares y + 1 < home).
            3 if y + 1 < self.home_y => {
                self.y += PLANT_STEP;
                false
            }
            // 0x08055B34: wait out of the pipe; a fire plant spits on the way.
            2 => {
                let spits = if self.spits_twice { SPIT_TWICE_AT.contains(&timer) } else { timer == SPIT_AT };
                if fire && spits {
                    self.spit = Some(Fireball::spat(x, y, fx, fy));
                }
                timer == 0
            }
            _ => true,
        };
        // 0x08055914: next phase, timer 0x30 (0x60 for the fire plants).
        let wait = if self.kind < FIRE_PIRANHA { PLANT_WAIT } else { FIRE_PLANT_WAIT };
        self.state = if advance {
            EnemyState::Plant { phase: (phase + 1) & 3, timer: wait }
        } else {
            EnemyState::Plant { phase, timer }
        };
        // 0x08055C3C: the chomp counter runs every frame; out of the pipe, 0x08055B34 keeps
        // a fire plant's mouth shut (+0x3C = 0).
        self.chomp = if fire && phase == 2 { 0 } else { self.chomp.wrapping_add(1) };
    }


    /// Paragoomba main 0x08053E9C. `self.age` is its counter +0x3A after this frame's +1.
    fn step_paragoomba(&mut self, level: &Level, player: &Player) {
        // The object loop counts the timer down; the draw comes before the counters move.
        self.hop_timer = self.hop_timer.saturating_sub(1);
        self.wings_drawn = self.wings;
        // 0x08049EC0 gravity, then one less (+0x12 - 1).
        self.vy = (self.vy + GRAVITY).min(MAX_FALL) - HOP_LIFT;
        let wall = self.move_x(level);
        let landed = self.drop(level);
        // Wings beat faster while it rises from the high hop.
        if self.hops == 0 && self.vy < 0 {
            self.wings = self.wings.wrapping_add(2);
        }
        self.wings = self.wings.wrapping_add(1);
        if landed {
            if self.age & PARA_TURN_MASK == 0 {
                self.vx = toward(player, self.px().0) * WALK_SPEED;
            }
            if self.hops == 0 {
                self.wings = 0;
            }
            if self.hop_timer == 0 {
                self.hops += 1;
                if self.hops == PARA_HOPS {
                    self.hops = 0;
                    self.hop_timer = PARA_REST;
                    self.vy = PARA_HIGH_HOP;
                } else {
                    self.vy = PARA_HOP;
                }
            }
        }
        // 0x0804A01C: walls turn it around.
        if wall {
            self.vx = -self.vx;
        }
    }

    /// Course clear (0x0806833C): a live enemy turns into a puff of smoke (its coin is
    /// made by the caller). Returns false if it was already gone.
    pub fn end_level(&mut self) -> bool {
        if !self.alive() {
            return false;
        }
        self.puff();
        true
    }

    /// Knocked out with no body left to fall (0x0804448C class 7 -> 0x08044460): state 8.
    fn puff(&mut self) {
        self.state = EnemyState::Puff(PUFF_TIME);
        self.vx = 0;
        self.vy = 0;
    }

    fn shell(&mut self, level: &Level, player: &Player, t: u8, frame: u32) {
        let t = if t >= SHELL_SLOW_TICKS && frame & 1 != 0 { t } else { t - 1 };
        if t == 0 {
            // Wakes up walking toward the player (0x08044720, direction 0x08169488).
            self.state = EnemyState::Walking;
            self.vx = toward(player, self.px().0) * WALK_SPEED;
            self.flipped = false;
            return;
        }
        self.state = EnemyState::Shell(t);
        if self.move_x(level) {
            self.vx = -self.vx;
        }
        if self.fall(level) {
            // Friction on the ground (0x08043984): the speed byte halves until it is tiny.
            let b = (self.vx / 16) >> 1;
            self.vx = if b == 0 || b == -1 { 0 } else { b * 16 };
        }
    }

    /// 0x080538FC: on walking off a ledge, turn around and step back onto it.
    fn walk_red(&mut self, level: &Level) {
        let (y, vy) = (self.y, self.vy);
        if self.move_x(level) {
            self.vx = -self.vx;
        }
        if self.fall(level) {
            self.at_ledge = false;
        } else if !self.at_ledge {
            self.at_ledge = true;
            self.vx = -self.vx;
            self.x += 2 * self.vx;
            self.y = y;
            self.vy = vy;
        }
    }

    /// Cell of the wall probe (0x08045FE4): offsets 0x08169E78[class | 4 (+2 moving right)]
    /// with class 0x0816A2BB[type] & 0xF8 = 8 for these enemies: (dy 9, dx 0) moving left,
    /// (dy 9, dx 15) moving right; taken once the move pushed the enemy out of the wall,
    /// so the probe point is one pixel further on.
    fn wall_probe(&self) -> (i32, i32) {
        let (px, py) = self.px();
        let x = if self.vx > 0 { px + 15 + 1 } else { px - 1 };
        (x.div_euclid(TILE_PX), (py + 9).div_euclid(TILE_PX))
    }

    /// Horizontal move; returns true if it ran into a wall (and was pushed out).
    fn move_x(&mut self, level: &Level) -> bool {
        self.x += self.vx;
        if self.vx == 0 {
            return false;
        }
        let (px, py) = self.px();
        let rows = (py + 2).div_euclid(TILE_PX)..=(py + SIZE - 2).div_euclid(TILE_PX);
        let front = if self.vx > 0 { px + SIZE - 1 } else { px };
        let tx = front.div_euclid(TILE_PX);
        if rows.into_iter().any(|ty| level.at(tx, ty) == TileKind::Solid) {
            self.x = if self.vx > 0 { (tx * TILE_PX - SIZE) * SUB } else { (tx + 1) * TILE_PX * SUB };
            return true;
        }
        false
    }

    /// Gravity and landing; returns true if standing on ground after the move.
    fn fall(&mut self, level: &Level) -> bool {
        self.vy = (self.vy + GRAVITY).min(MAX_FALL);
        self.drop(level)
    }

    /// Moves by `vy` and lands on the ground below.
    fn drop(&mut self, level: &Level) -> bool {
        let old_bottom = self.px().1 + SIZE;
        self.y += self.vy;
        let (px, py) = self.px();
        let ty = (py + SIZE).div_euclid(TILE_PX);
        let top = ty * TILE_PX;
        let cols = (px + 2).div_euclid(TILE_PX)..=(px + SIZE - 3).div_euclid(TILE_PX);
        let lands = cols.into_iter().any(|tx| match level.at(tx, ty) {
            TileKind::Solid => true,
            TileKind::Top => old_bottom <= top,
            TileKind::Empty => false,
        });
        if lands && self.vy >= 0 {
            self.y = (top - SIZE) * SUB;
            self.vy = 0;
            return true;
        }
        false
    }

    fn overlaps_box(&self, x: i32, y: i32, w: i32, h: i32) -> bool {
        let (ex, ey) = self.px();
        if self.is_plant() {
            let (dx, bw, dy) = PLANT_BOX;
            let bh = if self.kind == RED_PIRANHA { RED_PLANT_BOX_H } else { PLANT_BOX_H };
            return ex + dx < x + w && x < ex + dx + bw && ey + dy < y + h && y < ey + dy + bh;
        }
        ex + 2 < x + w && x < ex + SIZE - 2 && ey + 2 < y + h && y < ey + SIZE
    }

    fn overlaps(&self, player: &Player) -> bool {
        let (px, py) = player.px();
        self.overlaps_box(px, py, HITBOX_W, player.height())
    }

    /// Player contact for this frame.
    pub fn contact(&mut self, player: &Player) -> Contact {
        if !self.alive() || self.grace > 0 || !self.overlaps(player) {
            return Contact::None;
        }
        let feet = player.px().1 + player.height();
        let stomp = feet <= self.px().1 + STOMP_DEPTH;
        match self.state {
            // +0x64 bit 0x1000: 0x08048F80 sends even a landing to 0x08048D88, which hurts.
            EnemyState::Plant { .. } => Contact::HurtPlayer,
            EnemyState::Walking if stomp => {
                self.grace = STOMP_GRACE;
                match self.kind {
                    WALKER => {
                        self.state = EnemyState::Squished(SQUISH_FRAMES);
                        self.vx = 0;
                    }
                    PARATROOPA => {
                        self.kind = KOOPA;
                        self.vy = 0;
                    }
                    // 0x08048B3C: 0x0816966C[0x73] = 0x72, speed byte 0; the palette stays.
                    PARAGOOMBA => {
                        self.kind = WALKER;
                        self.vy = 0;
                        self.was_para = true;
                    }
                    _ => self.state = EnemyState::Shell(SHELL_TIMER),
                }
                Contact::Stomped
            }
            // Landing on or touching a resting shell kicks it (0x08048E58 / 0x08048E8C).
            EnemyState::Shell(_) => {
                self.kick(player);
                if stomp { Contact::KickedFromAbove } else { Contact::Kicked }
            }
            EnemyState::Kicked if stomp => {
                self.state = EnemyState::Shell(SHELL_TIMER);
                Contact::Stomped
            }
            // 0x08048D88: a sliding shell only hurts when it moves toward the player.
            EnemyState::Kicked if (self.vx > 0) != (toward(player, self.px().0) > 0) => Contact::None,
            _ => Contact::HurtPlayer,
        }
    }

    fn kick(&mut self, player: &Player) {
        let mut speed = -toward(player, self.px().0) * KICK_SPEED;
        let pv = player.vx / 16;
        if speed * pv > 0 {
            speed += pv >> 1;
        }
        self.vx = speed * 16;
        self.vy = 0;
        self.state = EnemyState::Kicked;
        self.grace = KICK_GRACE;
        self.knockouts = 0;
    }

    /// The raccoon's tail box `(x, y, w, h)` touched this enemy (0x08049970 -> 0x080496B8
    /// -> 0x0804955C): Goombas are knocked out; Koopas flip over into their shell (state 3,
    /// timer 0xFF, wings lost). Both hop up with speed byte 0xD0 and move 8/16 px per frame
    /// away from the player (0x08169D80).
    pub fn tail_hit(&mut self, (x, y, w, h): (i32, i32, i32, i32), player: &Player) -> bool {
        if !self.alive() || self.grace > 0 || !self.overlaps_box(x, y, w, h) {
            return false;
        }
        // 0x080496B8: a plant hidden in its pipe (+0x14 set by main) is out of reach
        // (0x0816AAF7 bit 0x80); otherwise 0x0804955C knocks it out into a puff.
        if self.is_plant() {
            if !self.plant_visible() {
                return false;
            }
            self.puff();
            return true;
        }
        let away = -toward(player, self.px().0);
        if self.is_koopa() {
            if self.kind == PARATROOPA {
                self.kind = KOOPA;
            }
            self.state = EnemyState::Shell(SHELL_TIMER);
            self.flipped = true;
        } else {
            self.state = EnemyState::Dead;
        }
        self.vy = KNOCKOUT_SPEED;
        self.vx = away * TAIL_DRIFT;
        self.grace = TAIL_GRACE;
        true
    }

    /// Knocked out by a shell moving in direction `dir`.
    fn knock_out(&mut self, dir: i32) {
        if self.is_plant() {
            self.puff();
            return;
        }
        self.state = EnemyState::Dead;
        self.vy = KNOCKOUT_SPEED;
        self.vx = dir * KNOCKOUT_DRIFT;
    }

    /// Sprite cell for this frame: the Goomba's `walker.png` or the Koopa sheet. `frame` is
    /// the global frame counter (the spinning shell uses 0x03003CF6 >> 1).
    pub fn sprite_cell(&self, frame: u32) -> usize {
        match (self.state, self.kind) {
            // 0x0805564C: frame (+0x3C >> 3) & 1, +2 (no stem) while less than 0x11 px out.
            (EnemyState::Plant { .. }, _) => {
                let low = if self.home_y - self.px().1 < PLANT_LOW { PIRANHA_LOW } else { 0 };
                let frame = ((self.chomp >> 3) & 1) as usize + low;
                if self.is_fire_plant() {
                    sma4_assets::fire_piranha_cell(self.looks_right, self.looks_up, frame)
                } else {
                    frame
                }
            }
            (EnemyState::Puff(t), _) => (PUFF_FRAMES as usize - 1).saturating_sub((t >> 3) as usize),
            (EnemyState::Squished(_), _) => WALKER_SQUISHED,
            (EnemyState::Walking, PARATROOPA) => ((self.age >> 2) & 3) as usize,
            // 0x08053E9C: body frame (frame counter >> 3) & 1; 0x08053D88 wings 0x107 while
            // +0x3C bit 3 is set, else 0x108.
            (EnemyState::Walking, PARAGOOMBA) => {
                sma4_assets::paragoomba_cell(((frame >> 3) & 1) as usize, self.wings_drawn & 8 != 0)
            }
            (EnemyState::Walking, _) => (self.age / 8 % 2) as usize,
            (EnemyState::Shell(t), _) => {
                let legs = t < SHELL_LEGS_BELOW && t & SHELL_LEG_MASKS[(t >> 4) as usize] != 0;
                if legs { KOOPA_SHELL_WAKING } else { KOOPA_SHELL }
            }
            (EnemyState::Kicked, _) => KOOPA_SPIN + ((frame >> 1) & 3) as usize,
            (EnemyState::Dead, WALKER) => 0,
            (EnemyState::Dead, _) => KOOPA_SHELL,
        }
    }
}

/// Kicked shells knock out enemies they touch; two sliding shells knock each other out.
/// Checked every other frame, like 0x08042C10. Returns (knocked-out enemy's top-left,
/// the shell's knockout count before this one) for the score popups (0x08093654 with the
/// shell's +0x0E counter).
pub fn shell_hits(enemies: &mut [Enemy], frame: u32) -> Vec<((i32, i32), u16)> {
    let mut out = Vec::new();
    if frame & 1 == 0 {
        return out;
    }
    for i in 0..enemies.len() {
        if enemies[i].state != EnemyState::Kicked {
            continue;
        }
        let (sx, sy) = enemies[i].px();
        let dir = enemies[i].vx.signum();
        for j in 0..enemies.len() {
            if i == j || !enemies[j].alive() || !enemies[j].overlaps_box(sx + 2, sy, SIZE - 4, SIZE) {
                continue;
            }
            if enemies[j].state == EnemyState::Kicked {
                enemies[i].knock_out(-dir);
            }
            enemies[j].knock_out(dir);
            out.push((enemies[j].px(), enemies[i].knockouts));
            enemies[i].knockouts += 1;
        }
    }
    out
}

/// Enemy list of a level, spawning entries as the player gets near them.
#[derive(Debug, Clone, Default)]
pub struct Spawner {
    pending: Vec<EnemySpawn>,
}

impl Spawner {
    pub fn new(list: &[EnemySpawn]) -> Self {
        // TODO(re): other enemy types.
        Self { pending: list.iter().copied().filter(|e| SUPPORTED.contains(&e.kind)).collect() }
    }

    /// Moves list entries that came within range into `active`.
    pub fn spawn_near(&mut self, player: &Player, active: &mut Vec<Enemy>) {
        let cx = player.centre().0;
        self.pending.retain(|s| {
            let ex = s.col as i32 * TILE_PX + SIZE / 2;
            if (ex - cx).abs() <= SPAWN_DISTANCE {
                active.push(Enemy::spawn(s, player));
                false
            } else {
                true
            }
        });
    }
}

/// A fireball spat by a Venus Fire Trap: projectile slot type 5 (0x03007848 + 0x8EC +
/// i * 0x18), handler 0x0808D774 in table 0x08170BB4.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fireball {
    /// Top-left, 1/256 px (+0 / +4).
    pub x: i32,
    pub y: i32,
    /// Speeds in 1/16 px per frame (+0x0A / +0x08).
    pub vx: i32,
    pub vy: i32,
    /// Frame counter +0x13, +1 per frame: the spin is (counter >> 2) & 3.
    pub age: u8,
}

/// Fireball speeds 0x0816E618 (y) / 0x0816E61A (x): [near, far]; far when the player is
/// 0x50 px or more away ((u8)(dx + 0x50) > 0x9F).
const FIRE_VY: [i32; 2] = [0x0B, 0x05];
const FIRE_VX: [i32; 2] = [0x0B, 0x0E];
const FIRE_FAR: u8 = 0x50;
/// Spawn point (0x08055938): plant x + 3 px, plant y (+16 for ceiling plants).
const FIRE_DX: i32 = 3;
/// Projectiles a fire plant can use: 0x0808C950 searches slots 3 down to 0.
pub const FIREBALL_SLOTS: usize = 4;
/// Player contact (0x0808CC9C): (fy - player y - 0x08170A9C[big] + 8) as u16 below
/// 0x08170A9E[big], and (fx - player x + 6) as u16 below 0x10; big = not small.
const FIRE_TOP: [i32; 2] = [0x12, 0x0A];
const FIRE_H: [i32; 2] = [0x10, 0x16];
const FIRE_W: i32 = 0x10;

impl Fireball {
    /// 0x08055938: aimed at the player whose 16x32 frame is at (fx, fy) from a plant at
    /// (x, y), all in px.
    fn spat(x: i32, y: i32, fx: i32, fy: i32) -> Self {
        let far = ((fx - x) as u8).wrapping_add(FIRE_FAR) > 0x9F;
        let far = far as usize;
        let up = (fy - y) < 0;
        let left = (fx - x) < 0;
        Self {
            x: (x + FIRE_DX) * SUB,
            y: y * SUB,
            vx: if left { -FIRE_VX[far] } else { FIRE_VX[far] },
            vy: if up { -FIRE_VY[far] } else { FIRE_VY[far] },
            age: 0,
        }
    }

    pub fn px(&self) -> (i32, i32) {
        (self.x >> 8, self.y >> 8)
    }

    /// One frame (0x0808FB08 -> 0x0808D774). Returns false once it has left the screen
    /// (x more than 8 px left of the view or past 0xF8, y outside -0x14..0xEC), which the
    /// dispatcher tests before moving it.
    pub fn step(&mut self, camera: (i32, i32)) -> bool {
        let (x, y) = self.px();
        let (sx, sy) = (x - camera.0, y - camera.1);
        if !(-8..0xF8).contains(&sx) || !(-0x14..0xEC).contains(&sy) {
            return false;
        }
        self.age = self.age.wrapping_add(1);
        // 0x0808D0C4 / 0x0808D0D4: a straight line.
        self.x += self.vx * 16;
        self.y += self.vy * 16;
        true
    }

    /// Spin frame 0-3, a cell of `sma4_assets::fireball_path` (h-flipped cells follow for
    /// fireballs flying left).
    pub fn frame(&self) -> usize {
        ((self.age >> 2) & 3) as usize
    }

    /// 0x0808CC9C: touches the player.
    pub fn touches(&self, player: &Player) -> bool {
        let (x, y) = self.px();
        let (fx, fy) = player.frame();
        let big = (player.form != crate::player::Form::Small) as usize;
        let dy = (y - fy - FIRE_TOP[big] + 8) as u16;
        let dx = (x - fx + 6) as u16;
        dy < FIRE_H[big] as u16 && dx < FIRE_W as u16
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 30x8 level with ground on the last row, a wall at column 20 and a ledge: no ground
    /// under columns 24-25.
    fn level() -> Level {
        let (w, h) = (30, 8);
        let mut kinds = vec![TileKind::Empty; w * h];
        for x in 0..w {
            if !(24..26).contains(&x) {
                kinds[(h - 1) * w + x] = TileKind::Solid;
            }
        }
        kinds[6 * w + 20] = TileKind::Solid;
        Level::from_kinds(w, h, &kinds)
    }

    fn enemy(kind: u16, col: u16) -> EnemySpawn {
        EnemySpawn { kind, col, row: 6 }
    }

    fn far_player() -> Player {
        Player::spawn(0, 0)
    }

    fn run(e: &mut Enemy, lvl: &Level, p: &Player, frames: u32) {
        for f in 0..frames {
            e.step(lvl, p, f);
        }
    }

    #[test]
    fn spawns_facing_the_player_and_walks_half_a_pixel_per_frame() {
        let lvl = level();
        let player = Player::spawn(2 * TILE_PX, 6 * TILE_PX - 16);
        let mut e = Enemy::spawn(&enemy(WALKER, 8), &player);
        assert!(e.vx < 0, "player is to the left");
        let x0 = e.px().0;
        run(&mut e, &lvl, &player, 20);
        assert_eq!(x0 - e.px().0, 10);
    }

    #[test]
    fn turns_around_at_walls() {
        let lvl = level();
        let player = Player::spawn(25 * TILE_PX, 6 * TILE_PX - 16);
        let mut e = Enemy::spawn(&enemy(WALKER, 17), &player);
        assert!(e.vx > 0);
        run(&mut e, &lvl, &player, 200);
        assert!(e.vx < 0);
        assert!(e.px().0 + SIZE <= 20 * TILE_PX);
    }

    #[test]
    fn landing_on_top_squishes_a_goomba_for_16_frames() {
        let lvl = level();
        let p0 = far_player();
        let mut e = Enemy::spawn(&enemy(WALKER, 8), &p0);
        e.step(&lvl, &p0, 0);
        let (ex, ey) = e.px();
        // Player feet 4 px into the enemy's top.
        let player = Player::spawn(ex, ey + 4 - 16);
        assert_eq!(e.contact(&player), Contact::Stomped);
        let mut frames = 1;
        while e.step(&lvl, &p0, frames) {
            frames += 1;
        }
        assert_eq!(frames, SQUISH_FRAMES as u32);
    }

    #[test]
    fn touching_from_the_side_hurts_the_player() {
        let lvl = level();
        let p0 = far_player();
        let mut e = Enemy::spawn(&enemy(KOOPA, 8), &p0);
        e.step(&lvl, &p0, 0);
        let (ex, ey) = e.px();
        let player = Player::spawn(ex - 6, ey);
        assert_eq!(e.contact(&player), Contact::HurtPlayer);
    }

    #[test]
    fn green_koopa_walks_off_ledges_red_koopa_turns_back() {
        let lvl = level();
        let player = Player::spawn(29 * TILE_PX, 0);
        let mut green = Enemy::spawn(&enemy(KOOPA, 22), &player);
        let mut red = Enemy::spawn(&enemy(RED_KOOPA, 22), &player);
        run(&mut green, &lvl, &player, 120);
        run(&mut red, &lvl, &player, 120);
        assert!(green.px().1 > 6 * TILE_PX, "fell into the gap");
        assert_eq!(red.px().1, 6 * TILE_PX, "stayed on the ground");
        assert!(red.vx < 0, "turned around at the ledge");
    }

    #[test]
    fn stomped_koopa_becomes_a_shell_that_wakes_up_later() {
        let lvl = level();
        let p0 = far_player();
        let mut e = Enemy::spawn(&enemy(KOOPA, 8), &p0);
        e.step(&lvl, &p0, 0);
        let (ex, ey) = e.px();
        assert_eq!(e.contact(&Player::spawn(ex, ey + 4 - 16)), Contact::Stomped);
        assert_eq!(e.state, EnemyState::Shell(SHELL_TIMER));
        run(&mut e, &lvl, &p0, 30);
        assert_eq!(e.vx, 0, "friction stops it");
        // (0xFF - 0x5F) ticks every other frame, then 0x60 every frame.
        let mut frames = 0;
        while matches!(e.state, EnemyState::Shell(_)) {
            e.step(&lvl, &p0, frames);
            frames += 1;
        }
        assert!((410..=420).contains(&(frames + 30)), "woke after {} frames", frames + 30);
        assert_eq!(e.state, EnemyState::Walking);
    }

    #[test]
    fn touching_a_shell_kicks_it_away_and_it_knocks_out_enemies() {
        let lvl = level();
        let p0 = far_player();
        let mut shell = Enemy::spawn(&enemy(KOOPA, 4), &p0);
        shell.state = EnemyState::Shell(SHELL_TIMER);
        shell.vx = 0;
        shell.step(&lvl, &p0, 0);
        let (sx, sy) = shell.px();
        // Player on the left touches it: it slides right at 3 px per frame.
        let player = Player::spawn(sx - 8, sy);
        assert_eq!(shell.contact(&player), Contact::Kicked);
        assert_eq!(shell.vx, KICK_SPEED * 16);
        assert_eq!(shell.contact(&player), Contact::None, "grace period");

        let mut goomba = Enemy::spawn(&enemy(WALKER, 10), &p0);
        let mut all = [shell, goomba];
        for f in 0..60 {
            for e in all.iter_mut() {
                e.step(&lvl, &p0, f);
            }
            shell_hits(&mut all, f);
        }
        goomba = all[1];
        assert_eq!(goomba.state, EnemyState::Dead);
        assert_eq!(all[0].state, EnemyState::Kicked);
    }

    #[test]
    fn stomping_a_paratroopa_takes_its_wings() {
        let lvl = level();
        let p0 = far_player();
        let mut e = Enemy::spawn(&enemy(PARATROOPA, 8), &p0);
        run(&mut e, &lvl, &p0, 3);
        assert!(e.vy < 0, "hops after landing");
        let (ex, ey) = e.px();
        assert_eq!(e.contact(&Player::spawn(ex, ey + 4 - 16)), Contact::Stomped);
        assert_eq!((e.kind, e.state), (KOOPA, EnemyState::Walking));
    }

    /// Red Piranha Plant in a pipe whose top is row 6, columns 10-11.
    fn red_plant() -> Enemy {
        Enemy::spawn(&EnemySpawn { kind: RED_PIRANHA, col: 10, row: 6 }, &far_player())
    }

    fn phase(e: &Enemy) -> u8 {
        match e.state {
            EnemyState::Plant { phase, .. } => phase,
            s => panic!("not a plant: {s:?}"),
        }
    }

    #[test]
    fn piranha_plant_rises_waits_and_sinks_back() {
        let lvl = level();
        let p = far_player();
        let mut e = red_plant();
        assert_eq!(e.px(), (10 * TILE_PX + PLANT_X, 6 * TILE_PX));
        assert!(!e.plant_visible());
        // Timer 0 at spawn and the player far away: out straight away, 1 px per frame.
        e.step(&lvl, &p, 0);
        assert_eq!(phase(&e), 1);
        let mut f = 1;
        while phase(&e) == 1 {
            e.step(&lvl, &p, f);
            f += 1;
        }
        assert_eq!(e.px().1, e.home_y - RED_PLANT_REACH);
        assert_eq!(f, 1 + RED_PLANT_REACH as u32 + 1, "33 px, then one frame to switch phase");
        let out = f;
        while phase(&e) == 2 {
            e.step(&lvl, &p, f);
            f += 1;
        }
        assert_eq!(f - out, PLANT_WAIT as u32);
        while phase(&e) == 3 {
            e.step(&lvl, &p, f);
            f += 1;
        }
        assert_eq!(e.px().1, e.home_y - 1, "stops 1 px above home");
        let hidden = f;
        while phase(&e) == 0 {
            e.step(&lvl, &p, f);
            f += 1;
        }
        assert_eq!(f - hidden, PLANT_WAIT as u32);
    }

    #[test]
    fn paragoomba_hops_three_times_low_then_high_then_rests() {
        let lvl = level();
        let p = far_player();
        let mut e = Enemy::spawn(&enemy(PARAGOOMBA, 8), &p);
        let ground = 6 * TILE_PX;
        // Record the take-off speeds and the frames spent on the ground in between.
        let mut hops = Vec::new();
        let mut grounded = 0;
        let mut rests = Vec::new();
        for f in 0..600 {
            let was = e.vy;
            e.step(&lvl, &p, f);
            if e.vy < 0 && was >= 0 {
                hops.push(e.vy);
                rests.push(grounded);
                grounded = 0;
            } else if e.px().1 == ground && e.vy == 0 {
                grounded += 1;
            }
        }
        assert_eq!(&hops[..5], [PARA_HOP, PARA_HOP, PARA_HOP, PARA_HIGH_HOP, PARA_HOP]);
        // After the high hop it waits out the 0x50 timer (minus its air time) on the ground.
        assert!(rests[4] > 20, "rests after the high hop: {rests:?}");
        assert!(rests[1..4].iter().all(|&r| r == 0), "low hops chain straight away: {rests:?}");
    }

    #[test]
    fn paragoomba_wings_beat_and_a_stomp_leaves_a_goomba() {
        let lvl = level();
        let p = far_player();
        let mut e = Enemy::spawn(&enemy(PARAGOOMBA, 8), &p);
        let mut ups = 0;
        for f in 0..200 {
            e.step(&lvl, &p, f);
            ups += (e.sprite_cell(f) & 1) as u32;
        }
        assert!(ups > 0, "the wings flap");
        // Stomp from above.
        let (x, y) = e.px();
        let mut player = Player::spawn(x + 2, y - 12);
        player.vy = 2 * SUB;
        assert_eq!(e.contact(&player), Contact::Stomped);
        assert_eq!((e.kind, e.state, e.was_para), (WALKER, EnemyState::Walking, true));
        assert_eq!(e.contact(&player), Contact::None, "not twice in one bounce (+0x24 = 8)");
        run(&mut e, &lvl, &p, STOMP_GRACE as u32);
        let (x, y) = e.px();
        let mut player = Player::spawn(x + 2, y - 12);
        player.vy = 2 * SUB;
        assert_eq!(e.contact(&player), Contact::Stomped, "a second stomp flattens the Goomba");
        assert!(matches!(e.state, EnemyState::Squished(_)));
    }

    #[test]
    fn fire_plant_waits_0x60_and_spits_once_with_its_mouth_shut() {
        let lvl = level();
        let p = far_player();
        let mut e = Enemy::spawn(&EnemySpawn { kind: FIRE_PIRANHA, col: 10, row: 6 }, &p);
        let mut f = 0;
        while phase(&e) != 2 {
            e.step(&lvl, &p, f);
            f += 1;
        }
        assert_eq!(e.px().1, e.home_y - PLANT_REACH);
        let mut spits = Vec::new();
        let mut frames = 0;
        while phase(&e) == 2 {
            e.step(&lvl, &p, f);
            assert_eq!(e.chomp, 0, "mouth shut while out");
            if let Some(b) = e.spit {
                spits.push((frames, b));
            }
            f += 1;
            frames += 1;
        }
        assert_eq!(frames, FIRE_PLANT_WAIT as u32);
        assert_eq!(spits.len(), 1);
        let (at, b) = spits[0];
        assert_eq!(at, (FIRE_PLANT_WAIT - SPIT_AT - 1) as u32, "at timer 0x28");
        assert!(!e.looks_right && e.looks_up, "the player is up on the left");
        // Far away (|dx| >= 0x50): flat and fast, toward the player.
        assert_eq!((b.vx, b.vy), (-0x0E, -0x05));
        assert_eq!(b.px(), (e.px().0 + 3, e.px().1));
        // Hidden for 0x60 frames too.
        let mut hidden = 0;
        while phase(&e) != 0 {
            e.step(&lvl, &p, f);
            f += 1;
        }
        while phase(&e) == 0 {
            e.step(&lvl, &p, f);
            f += 1;
            hidden += 1;
        }
        assert_eq!(hidden, FIRE_PLANT_WAIT as u32);
    }

    #[test]
    fn fire_plant_spits_twice_after_world_1() {
        let lvl = level();
        let p = far_player();
        let mut e = Enemy::spawn(&EnemySpawn { kind: FIRE_PIRANHA, col: 10, row: 6 }, &p);
        e.spits_twice = true;
        let mut spits = 0;
        for f in 0..0x100 {
            e.step(&lvl, &p, f);
            spits += e.spit.is_some() as u32;
        }
        assert_eq!(spits, 2);
    }

    #[test]
    fn fireballs_aim_steeply_up_close_and_hurt_on_touch() {
        // Plant at (100, 80); player frame 32 px to the right, below it: near shot.
        let p = Player::spawn(132 - 6 + 8, 96);
        let (fx, fy) = p.frame();
        let mut b = Fireball::spat(100, 80, fx, fy);
        assert_eq!((b.vx, b.vy), (0x0B, 0x0B), "near: 11/16 px a frame each way");
        assert_eq!(b.px(), (103, 80));
        let camera = (0, 0);
        for _ in 0..16 {
            assert!(b.step(camera));
        }
        assert_eq!(b.px(), (103 + 11, 80 + 11), "16 frames x 11/16 px");
        assert_eq!(b.frame(), 0);
        b.step(camera);
        // Touch box: (fx - x + 6) < 16 and small: y - fy in 10..26.
        b.x = (fx + 2) * SUB;
        b.y = (fy + 12) * SUB;
        assert!(b.touches(&p));
        b.y = (fy + 30) * SUB;
        assert!(!b.touches(&p));
        // Off screen: gone before it moves.
        b.x = 0xF8 * SUB;
        assert!(!b.step(camera));
    }

    #[test]
    fn piranha_plant_stays_in_while_the_player_is_close() {
        let lvl = level();
        let mut e = red_plant();
        // Standing on the pipe, 20 px from its centre.
        let (cx, _) = (e.px().0 + SIZE / 2, 0);
        let near = Player::spawn(cx + 20 - HITBOX_W / 2, 0);
        run(&mut e, &lvl, &near, 200);
        assert_eq!(phase(&e), 0);
        let away = Player::spawn(cx + 40 - HITBOX_W / 2, 0);
        e.step(&lvl, &away, 0);
        assert_eq!(phase(&e), 1);
    }

    #[test]
    fn piranha_plant_hurts_even_from_above_and_the_tail_turns_it_to_smoke() {
        let lvl = level();
        let p = far_player();
        let mut e = red_plant();
        run(&mut e, &lvl, &p, 40);
        assert_eq!(phase(&e), 2);
        let (ex, ey) = e.px();
        assert_eq!(e.contact(&Player::spawn(ex, ey + 4 - 16)), Contact::HurtPlayer);
        assert!(e.tail_hit((ex, ey + 4, 12, 15), &p));
        assert_eq!(e.state, EnemyState::Puff(PUFF_TIME));
        assert_eq!(e.contact(&Player::spawn(ex, ey)), Contact::None);
        let mut frames = 1;
        while e.step(&lvl, &p, frames) {
            frames += 1;
        }
        assert_eq!(frames, PUFF_TIME as u32);
    }

    #[test]
    fn hidden_piranha_plant_ignores_the_tail() {
        let mut e = red_plant();
        let (ex, ey) = e.px();
        assert!(!e.tail_hit((ex, ey, 16, 16), &far_player()));
    }

    #[test]
    fn spawner_releases_supported_enemies_near_the_player_once() {
        let mut sp = Spawner::new(&[enemy(WALKER, 5), enemy(WALKER, 28), enemy(0xA6, 6), enemy(KOOPA, 7)]);
        let player = Player::spawn(0, 0);
        let mut active = Vec::new();
        sp.spawn_near(&player, &mut active);
        assert_eq!(active.len(), 2);
        sp.spawn_near(&player, &mut active);
        assert_eq!(active.len(), 2);
    }
}
