//! Enemies, reimplemented from the ROM's object routines (see docs/re-notes.md,
//! "Walking enemy" and "Koopa Troopa").
//!
//! * Goomba 0x72, green Koopa Troopa 0x6C and green Paratroopa 0x6E share main 0x08053704:
//!   speed byte 0x0816E586 = +/-8 (1/16 px per frame), turn at walls, walk off ledges;
//!   the Paratroopa hops. Red Koopa Troopa 0x6D (main 0x080538FC) turns at ledges.
//! * Paragoomba 0x73 (main 0x08053E9C): walks like a Goomba, hops three times (speed byte
//!   0xF0), then high (0xD0) and rests 0x50 frames on the ground; stomped, it becomes a
//!   Goomba 0x72 that keeps its own palette.
//! * Micro-Goomba Paragoomba 0x74 (main 0x0805425C, phases 0x0816E5BC): walks, flies up
//!   until 0x38 px above the player, hovers 0x80 frames dropping a Micro-Goomba every
//!   0x20 frames (projectile type 6, `MicroGoomba`), drops back down and walks 0x80 frames.
//! * In slope tilesets (World 1-2's hills) the floor test is 0x08045ABC's: the feet probe's
//!   height map, following the slope down by +0x33 px when the floor drops away.
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
/// Buzzy Beetle (World 1-5): the walker main 0x08053704 and a Koopa's shell (0x0816A3E8
/// = 0x45 like the Koopas), but fire-proof (0x0816A516[0x70] = 0x201: bit 0x200 bursts
/// fireballs, 0x0808B2F0).
pub const BUZZY: u16 = 0x70;
/// Red Paratroopa (World 1-4): flies up and down in place (main 0x08053CDC).
pub const RED_PARATROOPA: u16 = 0x6F;
/// Paragoomba: hops and flutters; loses its wings when stomped.
pub const PARAGOOMBA: u16 = 0x73;
/// Brown Paragoomba that drops Micro-Goombas (World 1-2).
pub const MICRO_PARAGOOMBA: u16 = 0x74;
/// Boomerang Bro (World 1-3; main 0x08068F78).
pub const BOOMERANG_BRO: u16 = 0x82;
/// Enemy-list type of the Goomba pipe opening to the right (types >= 0x190 are
/// generators, 0x0804B768 -> 0x0804B460: generator kind = type - 0x18F = 6).
pub const GOOMBA_PIPE: u16 = 0x195;
/// Green Piranha Plant: comes 0x19 px out of its pipe.
pub const PIRANHA: u16 = 0xA0;
/// Red Piranha Plant: comes 0x21 px out.
pub const RED_PIRANHA: u16 = 0xA2;
/// Red Piranha Plant hanging from a ceiling pipe (World 1-5; flags 0x0816E56E = 1, init
/// 0x08055140: y + 16, +0x1A = 0x80).
pub const CEILING_PIRANHA: u16 = 0xA3;
/// Green Venus Fire Trap (SMB3's 0xA4, flags 0x80): comes 0x19 px out and spits fire.
pub const FIRE_PIRANHA: u16 = 0xA4;
/// Types that live in a pipe and are listed at its top row (ceiling variants 0xA1 / 0xA3 /
/// 0xA5 / 0xA7 and the red Venus Fire Trap 0xA6 are not ported yet).
pub const PIRANHA_TYPES: [u16; 4] = [PIRANHA, RED_PIRANHA, CEILING_PIRANHA, FIRE_PIRANHA];
/// Enemy types implemented so far.
pub const SUPPORTED: [u16; 13] = [
    WALKER,
    KOOPA,
    RED_KOOPA,
    PARATROOPA,
    RED_PARATROOPA,
    BUZZY,
    PARAGOOMBA,
    MICRO_PARAGOOMBA,
    BOOMERANG_BRO,
    PIRANHA,
    RED_PIRANHA,
    CEILING_PIRANHA,
    FIRE_PIRANHA,
];

/// 8/16 px per frame.
const WALK_SPEED: i32 = 8 * 16;
/// 0x0816E586[2..]: the walk speed while the last floor value (+0x3E) is negative.
const SLOPE_WALK_SPEED: i32 = 6 * 16;
/// Frames between two turns of a Goomba at a wall (+0x27).
const TURN_WAIT: u8 = 0x10;
/// Goomba walk cycle (main 0x08053704): +0x35 += 0x0816E581[(0xC02 + 2) & 0xFF] each frame,
/// +0x3A += 1 on the carry, frame (+0x3A >> 2) & 1. The ROM indexes with the floor value
/// 0xC02 (see `Enemy::floor`), -15..15, so the table runs past its five entries.
const WALK_STEP: [u8; 18] = [0xFF, 0xC0, 0x80, 0x60, 0x40, 0xF8, 0x08, 0xFA, 0x06, 0xFB, 0x05, 0xF0, 0x10, 0xF4, 0x0C, 0xF7, 0x09, 0x06];
/// The same table at (0xC02 + 2) & 0xFF = 0xF3..0xFF.
const WALK_STEP_WRAPPED: [u8; 13] = [0x99, 0x67, 0x05, 0x08, 0xA9, 0x67, 0x05, 0x08, 0xDD, 0x67, 0x05, 0x08, 0x35];
/// Slope steepness per shape (0x0816955C): -2 / +2 at 45 degrees, -1 / +1 gentle, 0 flat,
/// `NO_FLOOR` for shapes the enemies' floor test ignores (ceilings and the like).
const SLOPE_DIR: [i8; sma4_assets::SLOPE_SHAPES] =
    [0, -2, 2, 0, 0, NO_FLOOR, NO_FLOOR, 0, 0, NO_FLOOR, 0, NO_FLOOR, -1, -1, 1, 1, NO_FLOOR, NO_FLOOR, NO_FLOOR, NO_FLOOR, NO_FLOOR];
const NO_FLOOR: i8 = -0x80;

/// 0x0816955C: a slope shape's steepness, or `None` if it has no floor.
pub fn slope_dir(shape: u8) -> Option<i8> {
    let d = SLOPE_DIR[shape as usize];
    (d != NO_FLOOR).then_some(d)
}
/// 0x08045ABC lands an enemy on a slope once its feet are this deep in the tile, or at the
/// height map.
const SLOPE_DEEP: i32 = 0xB;
/// Ground under a slope (0x08045ABC -> 0x08045430): feet on one of these ids move up to
/// 1 px above the tile and test again, which is how an enemy climbs onto a slope.
const SLOPE_FILL_IDS: [u8; 4] = [0xA8, 0xAA, 0x37, 0xAB];
/// Probe offsets 0x08169E78 (dy, dx) for class 8 (Goombas): walls (9, 0) / (9, 15); in a
/// slope tileset, after a frame on the ground, (2, 1) / (2, 14) (class 0x68).
const WALL_PROBE: (i32, i32, i32) = (9, 0, 15);
const SLOPE_WALL_PROBE: (i32, i32, i32) = (2, 1, 14);
// Shared object gravity (0x08049EC0): speed byte += 0x08169D9E[0] = 3 each frame, capped
// at 0x08169D9C[0] = 64 while falling (1/16 px per frame; water uses 1 and 16, not ported).
const GRAVITY: i32 = 3 * 16;
const MAX_FALL: i32 = 64 * 16;
/// In water (+0x32): gravity 0x08169D9E[1] = 1, falling at most 0x08169D9C[1] = 0x10, and
/// rising at most 0x18 (0x08049EC0).
const WATER_GRAVITY: i32 = 16;
const WATER_MAX_FALL: i32 = 0x10 * 16;
const WATER_RISE: i32 = -0x18 * 16;
/// A sliding shell's speed byte while on the floor (0x080441A4).
const KICKED_FLOOR_SPEED: i32 = 0x0C * 16;
/// Paratroopa (0x08053704, type 0x6E): on landing the speed byte becomes 0xD0, and each
/// frame in the air it is lowered by one more (gravity 2 instead of 3).
const HOP_SPEED: i32 = -48 * 16;
const HOP_LIFT: i32 = 16;
/// Red Paratroopa top speed (0x0816E57A: speed byte +/-0x10) and the hold at each end.
const FLY_SPEED: i32 = 0x10 * 16;
const FLY_HOLD: u8 = 0x30;
/// Paragoomba (0x08053E9C): on the ground with timer +0x23 at 0, it hops (speed byte
/// 0xF0) and counts the hop in +0x3B; the fourth is high (0xD0), resets the count and
/// sets the timer to 0x50. Gravity is the Paratroopa's (+3 - 1).
const PARA_HOP: i32 = -16 * 16;
const PARA_HIGH_HOP: i32 = -48 * 16;
const PARA_HOPS: u8 = 4;
const PARA_REST: u8 = 0x50;
/// On the ground it turns toward the player when its counter +0x3A is a multiple of 0x40.
const PARA_TURN_MASK: u32 = 0x3F;
/// Micro-Goomba Paragoomba (0x0805425C): phase timer +0x23 = 0x80 on every phase change;
/// the wings rise for the last 0x20 frames on the ground. Phase 0 gravity is 0x08049EC0's
/// +3 (to 0x40) then -2.
const MICRO_PARA_PHASE: u8 = 0x80;
const MICRO_PARA_WINGS_UP: u8 = 0x20;
const MICRO_PARA_LIFT: i32 = 2 * 16;
const MICRO_PARA_MAX_FALL: i32 = 0x40 * 16;
/// Phase 1 rises at speed byte -0x10 (moved twice a frame) until 0x38 px above the player,
/// then slows by 1 a frame; phase 2 bobs at +/-8, flipping every 16 frames of +0x3A.
const MICRO_PARA_RISE: i32 = -0x10 * 16;
const MICRO_PARA_HEIGHT: u8 = 0x37;
const MICRO_PARA_BOB: i32 = 8 * 16;
/// Flying speed (0x08053FBC): toward +/-0x14 (0x0816E576) by 1 a frame (0x0816E574); faces
/// the player every 0x40 frames of +0x1D.
const MICRO_PARA_FLY: i32 = 0x14 * 16;
const MICRO_PARA_DROP_MASK: u8 = 0x1F;
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
/// Boomerang Bro: 16x32 (feet probe at y + 0x20), contact box 0x08169F44[2] = (2, 12, 2,
/// 24). Phases +0x3A & 3 (0x0816F058): stand (0x08068EA8), walk right at 8 (0x08068F5C),
/// stand, walk left (0x08068F68); a walk lasts 0x40 frames, a stand 0x10 (+0x34). Faces
/// the player every 16 frames (0x0816F068). Walking, the throw counter +0x3D throws when
/// (+0x3D & 0xBF) == 0x1F (+0x23 = 0xF: the throw pose) and it hops (speed 0xD0) when the
/// frame counter wraps.
const BRO_HEIGHT: i32 = 32;
const BRO_BOX: (i32, i32, i32, i32) = (2, 12, 2, 24);
const BRO_WALK: i32 = 8 * 16;
const BRO_WALK_TIME: u8 = 0x40;
const BRO_STAND_TIME: u8 = 0x10;
const BRO_HOP: i32 = -0x30 * 16;
const BRO_THROW_POSE: u8 = 0x0F;
/// Stomped, a Brother scores 4 steps up the chain (0x08169799[0x82] & 0xF4 == 4).
const BRO_STOMP_BONUS: u16 = 4;
/// Star knockout (0x080488E4): speed 0xC8 up, 0x081698C6 = -8 / 8 across.
const STAR_KNOCKOUT: i32 = -0x38 * 16;
const STAR_DRIFT: i32 = 8 * 16;
/// Fireball knockout (0x0808B2F0): speed 0xCC up, 0x0C across in the fireball's direction;
/// the box of Goombas and Koopas (0x0816A516[type] = 1 -> 0x08169F44[1]: dx 1, w 13, dy 2,
/// h 8).
const FIRE_KNOCKOUT: i32 = -0x34 * 16;
const FIRE_DRIFT: i32 = 0x0C * 16;
const FIRE_BOX: (i32, i32, i32, i32) = (1, 13, 2, 8);

/// Spawn column by scroll direction (0x08169DD0[0x03003AFE]): 0x110 px right of the camera
/// while it scrolls right, 0x30 px left of it while it scrolls left (0x0804BC6C).
const SPAWN_AHEAD: i32 = 0x110;
const SPAWN_BEHIND: i32 = -0x30;
/// Level start (0x0804C440): the camera stepped from camera - 0x10 to camera + 0x140 in
/// 16 px steps with the left-scroll offset, i.e. every column from camera - 0x40 to
/// camera + 0x110.
const START_FIRST: i32 = -0x10;
const START_LAST: i32 = 0x140;

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
    /// Floor value 0xC02 of this frame's floor test (0x08045ABC): on a slope tile, the
    /// feet's depth in the tile (y & 15, before the snap), negated while moving left; 0
    /// elsewhere. The Goomba keeps last frame's in +0x3E for its speed.
    floor: i8,
    floor_seen: i8,
    /// +0x33: px to follow the slope down when the floor drops away under the feet.
    slope_drop: i32,
    /// Stood on the ground last frame (0x03007848 + 0xBE7 bit 2): picks the wall probes.
    grounded: bool,
    /// Goomba turn cooldown +0x27 (main 0x08053704).
    turn_wait: u8,
    /// Goomba walk cycle: +0x35 accumulator and +0x3A counter.
    walk_acc: u8,
    walk_count: u8,
    /// Micro-Goomba Paragoomba: phase +0x3B, facing right (+0x1A bit 6), its flight turn
    /// counter +0x1D and the body frame its draw picked this frame (+0x17).
    phase: u8,
    pub facing_right: bool,
    drift: u8,
    body: u8,
    /// Micro-Goomba dropped this frame; the game puts it in a free projectile slot.
    pub dropped: Option<MicroGoomba>,
    /// A Goomba coming out of a Goomba pipe: frames left of +0x1C, during which it only
    /// moves across and is drawn behind the scenery (main 0x08053704, draw 0x08053664).
    pub emerging: u8,
    /// Made by a Goomba pipe: removed once off the screen (0x0804A0C4).
    pub from_pipe: bool,
    /// Boomerang Bro: phase timer +0x34 and the boomerang thrown this frame.
    wait: u8,
    pub thrown: Option<Boomerang>,
    /// Identity of this enemy (the boomerang finds its Bro by it).
    pub id: u32,
    /// This frame's random byte for its slot (0x03003879[slot]), set by the game.
    pub rand: u8,
    /// In water (+0x32; 0x080462A0 from the probe 0x08169E78[0x6C] = (4, 8)).
    pub in_water: bool,
    /// Went into or out of the water this frame (0x0804A224): the game makes the splash,
    /// and a bubble when it was falling in.
    pub splash: Option<bool>,
}

/// What touching an enemy did this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Contact {
    None,
    /// The player has the star: the enemy is knocked out (0x080488E4).
    StarKill,
    /// Landed on it: the player bounces.
    Stomped,
    /// Kicked a shell from the side.
    Kicked,
    /// Landed on a resting shell, kicking it (0x08048E58: chain points, no bounce).
    KickedFromAbove,
    HurtPlayer,
}

/// A new enemy identity.
fn next_id() -> u32 {
    use std::sync::atomic::{AtomicU32, Ordering};
    static NEXT: AtomicU32 = AtomicU32::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// Direction toward the player (0x08049F48 side test): +1 if the left of the player's
/// 16x32 frame is at or right of the object's `x` (1/256 px, subpixels included).
fn toward(player: &Player, x: i32) -> i32 {
    let frame_x = player.frame().0 * SUB + player.x.rem_euclid(SUB);
    if frame_x >= x { 1 } else { -1 }
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
            // The Bro's init (0x08068E80) leaves the speed at 0.
            vx: if kind == BOOMERANG_BRO { 0 } else { toward(player, x * SUB) * WALK_SPEED },
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
            floor: 0,
            floor_seen: 0,
            slope_drop: 0,
            grounded: false,
            turn_wait: 0,
            walk_acc: 0,
            walk_count: 0,
            phase: 0,
            facing_right: toward(player, x * SUB) > 0,
            drift: 0,
            body: 0,
            dropped: None,
            emerging: 0,
            from_pipe: false,
            wait: 0,
            thrown: None,
            id: next_id(),
            rand: 0,
            in_water: false,
            splash: None,
        }
    }

    /// A Piranha Plant in the pipe whose top is at pixel `(x, y)` (left column, top row).
    fn plant(kind: u16, x: i32, y: i32) -> Self {
        // 0x08055140: a ceiling plant starts 16 px lower, hanging below its pipe.
        let y = if kind == CEILING_PIRANHA { y + TILE_PX } else { y };
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
            reach: if matches!(kind, RED_PIRANHA | CEILING_PIRANHA) { RED_PLANT_REACH } else { PLANT_REACH },
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
            floor: 0,
            floor_seen: 0,
            slope_drop: 0,
            grounded: false,
            turn_wait: 0,
            walk_acc: 0,
            walk_count: 0,
            phase: 0,
            facing_right: false,
            drift: 0,
            body: 0,
            dropped: None,
            emerging: 0,
            from_pipe: false,
            wait: 0,
            thrown: None,
            id: next_id(),
            rand: 0,
            in_water: false,
            splash: None,
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

    /// Hangs from a ceiling pipe (flags 0x0816E56E bit 0; +0x1A bit 7).
    pub fn is_ceiling_plant(&self) -> bool {
        self.kind == CEILING_PIRANHA
    }

    /// Spits fire (flags 0x0816E56E bit 7).
    pub fn is_fire_plant(&self) -> bool {
        self.kind == FIRE_PIRANHA
    }

    pub fn is_koopa(&self) -> bool {
        matches!(self.kind, KOOPA | RED_KOOPA | PARATROOPA | RED_PARATROOPA | BUZZY)
    }

    /// Can hurt, be stomped or be knocked out.
    fn alive(&self) -> bool {
        !matches!(self.state, EnemyState::Squished(_) | EnemyState::Dead | EnemyState::Puff(_))
    }

    /// One frame. Returns false when the enemy should be removed.
    pub fn step(&mut self, level: &Level, player: &Player, frame: u32) -> bool {
        self.age = self.age.wrapping_add(1);
        self.grace = self.grace.saturating_sub(1);
        self.turn_wait = self.turn_wait.saturating_sub(1);
        self.wall_hit = None;
        self.spit = None;
        self.dropped = None;
        self.thrown = None;
        self.splash = None;
        match self.state {
            EnemyState::Squished(t) => {
                self.state = EnemyState::Squished(t - 1);
                return t > 1;
            }
            EnemyState::Walking if self.kind == RED_KOOPA => self.walk_red(level),
            EnemyState::Walking if self.kind == RED_PARATROOPA => self.fly_up_and_down(),
            EnemyState::Walking if self.kind == PARAGOOMBA => self.step_paragoomba(level, player),
            EnemyState::Walking if self.kind == MICRO_PARAGOOMBA => self.step_micro_paragoomba(level, player),
            EnemyState::Walking if self.kind == BOOMERANG_BRO => self.step_bro(level, player, frame),
            // 0x08053704 with +0x1C: out of the pipe, across only, the walk cycle +1 a frame.
            EnemyState::Walking if self.emerging > 0 => {
                self.emerging -= 1;
                self.x += self.vx;
                self.walk_count = self.walk_count.wrapping_add(1);
            }
            EnemyState::Walking => {
                // Main 0x08053704: speed 0x0816E586[direction, +2 if +0x3E < 0].
                if self.vx != 0 {
                    self.vx = self.vx.signum() * if self.floor_seen < 0 { SLOPE_WALK_SPEED } else { WALK_SPEED };
                }
                let wall = self.move_x(level);
                let landed = self.fall(level);
                if wall {
                    // With slopes the wall is only flagged; a Goomba (0x0816AAF7 bit 1)
                    // steps back out of it after the floor test, turning at most once
                    // every 0x10 frames (+0x27).
                    if level.has_slopes() && matches!(self.kind, WALKER | KOOPA | BUZZY) {
                        self.x -= self.vx;
                        if self.turn_wait == 0 {
                            self.turn_wait = TURN_WAIT;
                            self.vx = -self.vx;
                        }
                    } else {
                        self.vx = -self.vx;
                    }
                }
                if self.kind == PARATROOPA {
                    self.vy = if landed { HOP_SPEED } else { self.vy - HOP_LIFT };
                }
                self.floor_seen = self.floor;
                self.walk_cycle(landed);
            }
            EnemyState::Shell(t) => self.shell(level, player, t, frame),
            EnemyState::Kicked => {
                // 0x08042FF0: the wall probe's cell is hit like a head hit (0x08042EF8 ->
                // 0x0809FC1C(4)), then the shell turns around.
                // 0x080441A4: 0x08049EC0 moves across and down, then the probes (the wall
                // one in slope tilesets at the new y, before the floor snaps it); on the
                // floor the speed byte is set to 0x0C every frame, so the shell leaves a
                // ledge already falling (0x0C + 3).
                let landed = if level.has_slopes() {
                    self.x += self.water_vx();
                    let old_bottom = self.px().1 + self.height();
                    self.y += self.vy;
                    let wall = self.vx != 0 && self.slope_wall(level);
                    let landed = self.fall_moved(level, old_bottom);
                    if wall {
                        self.wall_hit = Some(self.wall_probe());
                        self.vx = -self.vx;
                    }
                    landed
                } else {
                    if self.move_x(level) {
                        self.wall_hit = Some(self.wall_probe());
                        self.vx = -self.vx;
                    }
                    self.fall(level)
                };
                if landed {
                    self.vy = KICKED_FLOOR_SPEED;
                }
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
        // Main 0x08055C88: a ceiling plant (+0x1A bit 7) runs handler (phase + 2) & 3: it
        // waits in its pipe with 0x08055B34, comes down with 0x08055C04 (to home), waits out
        // with 0x08055A24 and goes back up with 0x08055AFC; neither wait has the shy test
        // (flags bit 0) and a red one does not spit.
        let ceiling = self.is_ceiling_plant();
        let handler = if ceiling { (phase + 2) & 3 } else { phase };
        let advance = match handler {
            0 if ceiling => timer == 0,
            2 if ceiling => timer == 0,
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
        // 0x08049EC0: move, collide, gravity; then one less (+0x12 - 1).
        let wall = self.move_x(level);
        let old_bottom = self.px().1 + SIZE;
        self.y += self.vy;
        let landed = self.land(level, old_bottom);
        self.vy = (self.vy + GRAVITY).min(MAX_FALL) - HOP_LIFT;
        // Wings beat faster while it rises from the high hop.
        if self.hops == 0 && self.vy < 0 {
            self.wings = self.wings.wrapping_add(2);
        }
        self.wings = self.wings.wrapping_add(1);
        if landed {
            if self.age & PARA_TURN_MASK == 0 {
                self.vx = toward(player, self.x) * WALK_SPEED;
            }
            // 0x08045384 snaps it to the ground.
            self.vy = 0;
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

    /// Goomba walk cycle (main 0x08053704): the step depends on the floor value; off the
    /// ground +0x3A is held at 1.
    fn walk_cycle(&mut self, landed: bool) {
        let i = (self.floor as i32 + 2) as u8;
        let step = match i {
            0..=17 => WALK_STEP[i as usize],
            0xF3.. => WALK_STEP_WRAPPED[(i - 0xF3) as usize],
            _ => 0,
        };
        let acc = self.walk_acc.wrapping_add(step);
        if acc <= self.walk_acc {
            self.walk_count = self.walk_count.wrapping_add(1);
        }
        self.walk_acc = acc;
        if !landed {
            self.walk_count = 1;
        }
    }

    /// Red Paratroopa main 0x08053CDC: +0x3A counts frames; y += speed (0x08049E80); with
    /// the timer +0x23 out, every 4th frame (+0x3B) the speed byte steps by
    /// 0x0816E578[+0x3C & 1] (+1 / -1) and on reaching 0x0816E57A[+0x3C & 1] (0x10 / -0x10)
    /// the direction flips and the timer is 0x30, during which it keeps that speed.
    fn fly_up_and_down(&mut self) {
        self.walk_count = self.walk_count.wrapping_add(1);
        self.y += self.vy;
        self.hop_timer = self.hop_timer.saturating_sub(1);
        if self.hop_timer != 0 {
            return;
        }
        self.hops = self.hops.wrapping_add(1);
        if self.hops & 3 == 0 {
            let up = self.wings & 1 != 0;
            self.vy += if up { -16 } else { 16 };
            if self.vy == if up { -FLY_SPEED } else { FLY_SPEED } {
                self.wings = self.wings.wrapping_add(1);
                self.hop_timer = FLY_HOLD;
            }
        }
    }

    /// Micro-Goomba Paragoomba main 0x0805425C. The draw (body frame from +0x3A & 8, wings
    /// from +0x3C) runs before the phase handler.
    fn step_micro_paragoomba(&mut self, level: &Level, player: &Player) {
        self.hop_timer = self.hop_timer.saturating_sub(1);
        self.wings_drawn = self.wings;
        // TODO(re): the mask is 0x0816E57E[0xC02 of the last floor test], 8 off slopes.
        self.body = (self.walk_count & 8 != 0) as u8;
        let old_bottom = self.px().1 + SIZE;
        match self.phase {
            // 0x08054084: walk like a Goomba, take off when the timer runs out.
            0 => {
                self.walk_count = self.walk_count.wrapping_add(1);
                self.vx = if self.facing_right { WALK_SPEED } else { -WALK_SPEED };
                let wall = self.move_x(level);
                self.y += self.vy;
                let landed = self.land(level, old_bottom);
                // 0x08049EC0: gravity after the move, then 2 less.
                self.vy = (self.vy + GRAVITY).min(MICRO_PARA_MAX_FALL) - MICRO_PARA_LIFT;
                if landed {
                    if self.walk_count as u32 & PARA_TURN_MASK == 0 {
                        self.facing_right = toward(player, self.x) > 0;
                    }
                    self.vy = 0;
                    if self.hop_timer == 0 {
                        self.phase = 1;
                    } else {
                        self.wings = if self.hop_timer >= MICRO_PARA_WINGS_UP { 0 } else { 8 };
                    }
                } else {
                    self.wings = 8;
                }
                if wall {
                    self.facing_right = !self.facing_right;
                }
            }
            // 0x08054120: rise (moved twice a frame) until 0x38 px above the player.
            1 => {
                self.wings = self.wings.wrapping_add(3);
                let dy = player.frame().1 - self.px().1;
                if dy >= 0 && dy as u8 > MICRO_PARA_HEIGHT {
                    let was = self.vy;
                    self.vy += 16;
                    if was == -16 {
                        self.next_phase();
                        return;
                    }
                } else {
                    self.vy = MICRO_PARA_RISE;
                }
                self.y += self.vy;
                self.fly(level, player, old_bottom);
            }
            // 0x080541C4: hover, bobbing, dropping a Micro-Goomba every 0x20 frames.
            _ => {
                if self.hop_timer == 0 {
                    self.phase = 0;
                    self.hop_timer = MICRO_PARA_PHASE;
                }
                self.wings = self.wings.wrapping_add(2);
                self.walk_count = self.walk_count.wrapping_add(1);
                if self.walk_count & MICRO_PARA_DROP_MASK == 0 {
                    // 0x08054028: projectile type 6 at the body + (4, 4).
                    self.dropped = Some(MicroGoomba::new(self.x + 4 * SUB, self.y + 4 * SUB));
                }
                self.vy = if self.walk_count & 0x10 == 0 { MICRO_PARA_BOB } else { -MICRO_PARA_BOB };
                self.fly(level, player, old_bottom);
            }
        }
    }

    /// Boomerang Bro main 0x08068F78. The object loop counts +0x23 and +0x34 down first.
    fn step_bro(&mut self, level: &Level, player: &Player, frame: u32) {
        self.hop_timer = self.hop_timer.saturating_sub(1);
        self.wait = self.wait.saturating_sub(1);
        // 0x08049EC0: move (walls are only flagged), fall, land, then gravity.
        self.x += self.vx;
        if self.drop(level) {
            self.vy = 0;
        } else {
            self.vy = (self.vy + GRAVITY).min(MAX_FALL);
        }
        if frame & 0xF == 0 {
            self.facing_right = toward(player, self.x) > 0;
        }
        match self.phase & 3 {
            // 0x08068EA8: stand; then walk for 0x40 frames.
            0 | 2 => {
                if self.wait == 0 {
                    self.phase = self.phase.wrapping_add(1);
                    self.wait = BRO_WALK_TIME;
                }
                self.vx = 0;
            }
            // 0x08068ECC: walk right / left.
            p => {
                self.vx = if p == 1 { BRO_WALK } else { -BRO_WALK };
                self.walk_count = self.walk_count.wrapping_add(2);
                // When the frame counter wraps and the slot's random byte has bit 7.
                if frame & 0xFF == 0 && self.rand & 0x80 != 0 {
                    self.vy = BRO_HOP;
                }
                if self.wait == 0 {
                    self.phase = self.phase.wrapping_add(1);
                    self.wait = BRO_STAND_TIME;
                }
                let c = self.drift;
                self.drift = self.drift.wrapping_add(1);
                if c & 0xBF == 0x1F {
                    self.hop_timer = BRO_THROW_POSE;
                    self.thrown = Some(Boomerang::thrown(self, player));
                }
            }
        }
    }

    /// Per-type init that reads the slot's random byte: the Boomerang Bro starts in phase
    /// 0x03003879[slot] (0x08068E80).
    pub fn init_random(&mut self, rand: u8) {
        if self.kind == BOOMERANG_BRO {
            self.phase = rand;
        }
    }

    /// Points step bonus for a stomp (0x08048B3C).
    pub fn stomp_bonus(&self) -> u16 {
        if self.kind == BOOMERANG_BRO { BRO_STOMP_BONUS } else { 0 }
    }

    /// +0x3B += 1 with the phase timer at 0x80.
    fn next_phase(&mut self) {
        self.phase += 1;
        self.hop_timer = MICRO_PARA_PHASE;
    }

    /// 0x08053FBC and the flying phases' collision: speed toward 0x14 the way it faces, a
    /// turn toward the player every 0x40 frames of +0x1D, move; land (back to phase 0),
    /// walls (reverse), ceiling (stop rising and hover).
    fn fly(&mut self, level: &Level, player: &Player, old_bottom: i32) {
        let target = if self.facing_right { MICRO_PARA_FLY } else { -MICRO_PARA_FLY };
        if self.vx != target {
            self.vx += if self.facing_right { 16 } else { -16 };
        }
        self.drift = self.drift.wrapping_add(1);
        if self.drift & 0x3F == 0 {
            self.facing_right = toward(player, self.x) > 0;
        }
        let wall = self.move_x(level);
        self.y += self.vy;
        if self.land(level, old_bottom) {
            self.vy = 0;
            self.phase = 0;
            self.hop_timer = MICRO_PARA_PHASE;
        }
        if wall {
            self.vx = -self.vx;
            self.facing_right = !self.facing_right;
        }
        if self.vy < 0 && self.ceiling(level) {
            self.vy = 0;
            if self.phase == 1 {
                self.next_phase();
            }
        }
    }

    /// Head probe (0, 8) in a solid tile: pushed down below it.
    fn ceiling(&mut self, level: &Level) -> bool {
        let (px, py) = self.px();
        let ty = py.div_euclid(TILE_PX);
        if level.at((px + 8).div_euclid(TILE_PX), ty) != TileKind::Solid {
            return false;
        }
        self.y = (ty + 1) * TILE_PX * SUB;
        true
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
            self.vx = toward(player, self.x) * WALK_SPEED;
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

    /// The move across: in water anything but a walking enemy moves at half its speed byte
    /// (0x08049EC0).
    fn water_vx(&self) -> i32 {
        if self.in_water && self.state != EnemyState::Walking { (self.vx >> 4 >> 1) << 4 } else { self.vx }
    }

    /// Horizontal move; returns true if it ran into a wall (and was pushed out). In water,
    /// anything but a walking enemy moves at half its speed byte (0x08049EC0).
    fn move_x(&mut self, level: &Level) -> bool {
        self.x += self.water_vx();
        if self.vx == 0 {
            return false;
        }
        if level.has_slopes() {
            return self.slope_wall(level);
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

    /// Fall and land, then gravity (0x08049EC0: move, collide, speed += 3 up to 0x40);
    /// returns true if standing on ground after the move.
    /// In water the speed is held above -0x18, gravity is 1 and the fall at most 0x10.
    fn fall(&mut self, level: &Level) -> bool {
        let old_bottom = self.px().1 + self.height();
        self.y += self.vy;
        self.fall_moved(level, old_bottom)
    }

    /// `fall` once y has moved: land, water, gravity.
    fn fall_moved(&mut self, level: &Level, old_bottom: i32) -> bool {
        // 0x08046358 takes the water probe before the floor snaps the object.
        let probe = self.px();
        let landed = self.land(level, old_bottom);
        self.water(level, probe);
        if !landed {
            let (g, max) = if self.in_water { (WATER_GRAVITY, WATER_MAX_FALL) } else { (GRAVITY, MAX_FALL) };
            if self.in_water && self.vy < WATER_RISE {
                self.vy = WATER_RISE;
            }
            self.vy = (self.vy + g).min(max);
        }
        landed
    }

    /// 0x080462A0: the water flag from the probe (x + 8, y + 4), kept while the probe is in
    /// a colliding cell (0x08046358 only calls it for open ones); going in or out (not out of
    /// the bottom of the level) splashes, and stops a fall (0x0804A224).
    fn water(&mut self, level: &Level, (px, py): (i32, i32)) {
        let (tx, ty) = ((px + 8).div_euclid(TILE_PX), (py + 4).div_euclid(TILE_PX));
        if level.at(tx, ty) != TileKind::Empty {
            return;
        }
        let water = level.is_water(tx, ty);
        if water != self.in_water && py + 4 < level.pixel_height() {
            let falling = self.vy >= 0;
            if falling {
                self.vy = 0;
            }
            self.splash = Some(falling);
        }
        self.in_water = water;
    }

    /// The wall probe of a slope tileset (0x08045ABC): one point, high up the body after a
    /// frame on the ground so the slope underfoot never counts. Nothing is pushed out.
    fn slope_wall(&self, level: &Level) -> bool {
        let (dy, left, right) = if self.grounded { SLOPE_WALL_PROBE } else { WALL_PROBE };
        let (px, py) = self.px();
        let x = px + if self.vx > 0 { right } else { left };
        level.at(x.div_euclid(TILE_PX), (py + dy).div_euclid(TILE_PX)) == TileKind::Solid
    }

    /// Moves by `vy` and lands on the ground below.
    fn drop(&mut self, level: &Level) -> bool {
        let old_bottom = self.px().1 + self.height();
        self.y += self.vy;
        self.land(level, old_bottom)
    }

    /// Lands on the ground under the box if it is falling onto it; `old_bottom` is the
    /// bottom before this frame's move.
    fn land(&mut self, level: &Level, old_bottom: i32) -> bool {
        if level.has_slopes() {
            self.floor = 0;
            self.grounded = self.vy >= 0 && (self.slope_floor(level) || self.follow_slope_down(level));
            return self.grounded;
        }
        let (px, py) = self.px();
        let h = self.height();
        let ty = (py + h).div_euclid(TILE_PX);
        let top = ty * TILE_PX;
        let cols = (px + 2).div_euclid(TILE_PX)..=(px + SIZE - 3).div_euclid(TILE_PX);
        let lands = cols.into_iter().any(|tx| match level.at(tx, ty) {
            TileKind::Solid => true,
            TileKind::Top => old_bottom <= top,
            TileKind::Empty => false,
        });
        if lands && self.vy >= 0 {
            self.y = (top - h) * SUB;
            self.vy = 0;
            return true;
        }
        false
    }

    /// Feet below the object's y: 16, or 32 for the Brothers (probe offsets 0x08169E78,
    /// class 0x10: feet (0x20, 8)).
    pub fn height(&self) -> i32 {
        if self.kind == BOOMERANG_BRO { BRO_HEIGHT } else { SIZE }
    }

    /// 0x08045ABC's floor in a slope tileset: the feet probe (16, 8) in a tile at least
    /// top-solid, whose shape has a floor (0x0816955C != 0x80), lands the enemy once the
    /// feet reach the height map (or are more than 11 px into the tile), and snaps it there
    /// (0x08045384). It also sets the floor value 0xC02 and the slope follow +0x33 =
    /// (speed byte >> 4, or 1 when 0) down the slope.
    fn slope_floor(&mut self, level: &Level) -> bool {
        let (px, mut py) = self.px();
        let h = self.height();
        let (fx, mut ty) = (px + SIZE / 2, (py + h).div_euclid(TILE_PX));
        let tx = fx.div_euclid(TILE_PX);
        while level.collision_id(tx, ty).is_some_and(|id| SLOPE_FILL_IDS.contains(&id)) && ty > 0 {
            py = py.div_euclid(TILE_PX) * TILE_PX - 1;
            self.y = py * SUB;
            ty = (py + h).div_euclid(TILE_PX);
        }
        if level.at(tx, ty) == TileKind::Empty {
            return false;
        }
        let shape = level.slope(tx, ty);
        let dir = SLOPE_DIR[shape as usize];
        if dir == NO_FLOOR {
            return false;
        }
        let depth = py.rem_euclid(TILE_PX);
        let height = level.slope_height(shape, fx);
        if depth <= SLOPE_DEEP && depth < height {
            return false;
        }
        self.y = (py - depth + height) * SUB;
        self.vy = 0;
        let follow = match (self.vx / 16) >> 4 {
            0 => 1,
            v => v,
        };
        self.slope_drop = match dir {
            0 => 0,
            d if d < 0 => -follow,
            _ => follow,
        };
        self.floor = if self.vx < 0 { -depth as i8 } else { depth as i8 };
        true
    }

    /// 0x0804545C: no floor under the feet; once, move down by +0x33 px and test again
    /// (0x08045DF4), so an enemy walking down a slope stays on it.
    fn follow_slope_down(&mut self, level: &Level) -> bool {
        if self.slope_drop == 0 {
            return false;
        }
        self.y += self.slope_drop * SUB;
        self.slope_drop = 0;
        self.slope_floor(level)
    }

    fn overlaps_box(&self, x: i32, y: i32, w: i32, h: i32) -> bool {
        let (ex, ey) = self.px();
        if self.is_plant() {
            let (dx, bw, dy) = PLANT_BOX;
            let bh = if matches!(self.kind, RED_PIRANHA | CEILING_PIRANHA) { RED_PLANT_BOX_H } else { PLANT_BOX_H };
            return ex + dx < x + w && x < ex + dx + bw && ey + dy < y + h && y < ey + dy + bh;
        }
        if self.kind == BOOMERANG_BRO {
            let (dx, bw, dy, bh) = BRO_BOX;
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
        // 0x08048960: with the star, any touch knocks the object out (0x080488E4): state 6,
        // speed 0xC8 up and 0x081698C6[side] = -8 / 8 away from the player.
        if player.star != 0 {
            if self.is_plant() {
                self.puff();
            } else {
                self.state = EnemyState::Dead;
                self.flipped = false;
                self.vy = STAR_KNOCKOUT;
                self.vx = -toward(player, self.x) * STAR_DRIFT;
            }
            return Contact::StarKill;
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
                    // 0x0816966C[0x6F] = 0x6D.
                    RED_PARATROOPA => {
                        self.kind = RED_KOOPA;
                        self.vy = 0;
                    }
                    // 0x08048B3C: 0x0816966C[0x73] = 0x72, speed byte 0; the palette stays.
                    PARAGOOMBA => {
                        self.kind = WALKER;
                        self.vy = 0;
                        self.was_para = true;
                    }
                    // 0x08048B3C, 0x0816A2BB[type] bit 0: knocked out (state 6, sound 0x7F).
                    BOOMERANG_BRO => {
                        self.state = EnemyState::Dead;
                        self.vx = 0;
                        self.vy = 0;
                    }
                    // 0x0816966C[0x74] = 0x72 too; its palette is the Goomba's.
                    MICRO_PARAGOOMBA => {
                        self.kind = WALKER;
                        self.vy = 0;
                        self.vx = if self.facing_right { WALK_SPEED } else { -WALK_SPEED };
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
            EnemyState::Kicked if (self.vx > 0) != (toward(player, self.x) > 0) => Contact::None,
            _ => Contact::HurtPlayer,
        }
    }

    fn kick(&mut self, player: &Player) {
        let mut speed = -toward(player, self.x) * KICK_SPEED;
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
        let away = -toward(player, self.x);
        if self.is_koopa() {
            if self.kind == PARATROOPA {
                self.kind = KOOPA;
            }
            if self.kind == RED_PARATROOPA {
                self.kind = RED_KOOPA;
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

    /// A fireball at `(x, y)` px (8x8, flying in direction `dir`) hits this enemy
    /// (0x0808B2F0): box 0x08169F44[+0x64 & 0x3F] widened by 8 px to the left and 0xE px
    /// up. The enemy is knocked out (state 6, speed 0xCC up and 0x0C away; plants puff).
    /// A player fireball at `(x, y)`: None if it misses, Some(true) if it knocks the
    /// object out, Some(false) if it only bursts (a fire-proof object).
    pub fn fireball_hit(&mut self, (x, y): (i32, i32), dir: i32) -> Option<bool> {
        if !self.alive() || (self.is_plant() && !self.plant_visible()) {
            return None;
        }
        let (ex, ey) = self.px();
        let (dx, w, dy, h) = if self.is_plant() {
            let (dx, w, dy) = PLANT_BOX;
            (dx, w, dy, if matches!(self.kind, RED_PIRANHA | CEILING_PIRANHA) { RED_PLANT_BOX_H } else { PLANT_BOX_H })
        } else if self.kind == BOOMERANG_BRO {
            BRO_BOX
        } else {
            FIRE_BOX
        };
        let vertical = (y - (ey + dy) + 0xE) as u16 as i32;
        let across = (x - (ex + dx) + 8) as u16 as i32;
        if vertical >= h + 0xE || across >= w + 8 {
            return None;
        }
        if self.kind == BUZZY {
            return Some(false);
        }
        if self.is_plant() {
            self.puff();
        } else {
            self.state = EnemyState::Dead;
            self.flipped = false;
            self.vy = FIRE_KNOCKOUT;
            self.vx = dir * FIRE_DRIFT;
        }
        Some(true)
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
                // (A ceiling plant's draw flips this too: its +2 frames are the ones with
                // the stem, shown near home where it hangs out of its pipe.)
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
            // 0x08053CDC / 0x080539E8: cells by +0x3A >> 1 (body (+0x3A >> 3) & 1, wing by
            // (+0x3A + 2) & 4).
            (EnemyState::Walking, RED_PARATROOPA) => ((self.walk_count >> 1) & 7) as usize,
            // 0x08053E9C: body frame (frame counter >> 3) & 1; 0x08053D88 wings 0x107 while
            // +0x3C bit 3 is set, else 0x108.
            (EnemyState::Walking, PARAGOOMBA) => {
                sma4_assets::paragoomba_cell(((frame >> 3) & 1) as usize, self.wings_drawn & 8 != 0)
            }
            // 0x08068BB8: frame = walk step (+0x3B bit 4) + 2 while throwing (+0x23).
            (EnemyState::Walking, BOOMERANG_BRO) => sma4_assets::boomerang_bro_cell(
                self.facing_right,
                self.hop_timer != 0,
                ((self.walk_count >> 4) & 1) as usize,
            ),
            (EnemyState::Dead, BOOMERANG_BRO) => sma4_assets::boomerang_bro_cell(self.facing_right, false, 0),
            (EnemyState::Walking, MICRO_PARAGOOMBA) => sma4_assets::paragoomba_cell(self.body as usize, self.wings_drawn & 8 != 0),
            // 0x08053704: frame (+0x3A >> 2) & 1.
            (EnemyState::Walking, WALKER) => ((self.walk_count >> 2) & 1) as usize,
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

/// Goomba pipe (generator kind 6, handler 0x0809006C; slots 0x03007848 + 0xA54, eight,
/// newest first): every 0x70 frames, three times out of four, a Goomba comes out of the
/// pipe's mouth, 3 px above the entry's row, walking right for 0x28 frames before it
/// starts to fall and collide; only while the player is to the right of the pipe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GoombaPipe {
    /// The list entry's position, px.
    pub x: i32,
    pub y: i32,
    /// +0x0A: frames to the next Goomba (0x60 at first, 0x0804B460).
    timer: u8,
    /// +0x0C: Goombas tried so far; every fourth is skipped.
    count: u8,
}

/// Generator slots (0x08090448 runs eight).
pub const GENERATOR_SLOTS: usize = 8;
/// Goombas from pipes take object slots 4..0 (0x0804CB98).
pub const PIPE_GOOMBA_SLOTS: usize = 5;
const PIPE_FIRST: u8 = 0x60;
const PIPE_EVERY: u8 = 0x70;
const PIPE_EMERGE: u8 = 0x28;
const PIPE_RISE: i32 = 3;
/// Off-screen removal (0x0804A0C4, table 0x08169DA8): more than 0x120 px right of the
/// camera, or 0x40 px left of it.
const OFFSCREEN_RIGHT: i32 = 0x120;
const OFFSCREEN_LEFT: i32 = 0x40;

impl GoombaPipe {
    pub fn new(s: &EnemySpawn) -> Self {
        Self { x: s.col as i32 * TILE_PX, y: s.row as i32 * TILE_PX, timer: PIPE_FIRST, count: 0 }
    }

    /// One frame (the loop counts the timer down first). `room` is whether one of the
    /// five object slots is free.
    pub fn step(&mut self, player: &Player, room: bool) -> Option<Enemy> {
        self.timer = self.timer.saturating_sub(1);
        if self.timer != 0 {
            return None;
        }
        self.timer = PIPE_EVERY;
        self.count = self.count.wrapping_add(1);
        // 0x08049F48 must say the player is right of the mouth (kind 6; kind 5 = left).
        if self.count & 3 == 0 || !room || toward(player, self.x * SUB) < 0 {
            return None;
        }
        let mut e = Enemy::spawn_at(WALKER, self.x, self.y - PIPE_RISE, player);
        e.vx = WALK_SPEED;
        e.emerging = PIPE_EMERGE;
        e.from_pipe = true;
        Some(e)
    }
}

impl Enemy {
    /// 0x0804A0C4: off the screen for good.
    pub fn off_screen(&self, camera: (i32, i32)) -> bool {
        let x = self.px().0;
        x > camera.0 + OFFSCREEN_RIGHT || x < camera.0 - OFFSCREEN_LEFT
    }
}

/// Enemy list of a level, spawning entries as the camera reaches their column (0x0804C92C
/// -> 0x0804BC6C, every frame).
#[derive(Debug, Clone, Default)]
pub struct Spawner {
    pending: Vec<EnemySpawn>,
    /// Camera x last frame; `None` until the level-start sweep has run.
    camera_x: Option<i32>,
    /// Scrolling left (0x03003AFE = 1; set by the camera update 0x0809869C).
    left: bool,
    /// The goal card's column was reached.
    goal: bool,
    /// An autoscroll entry's column was reached: its row.
    autoscroll: Option<u8>,
    /// Lifts whose column was reached.
    lifts: Vec<EnemySpawn>,
}

impl Spawner {
    pub fn new(list: &[EnemySpawn]) -> Self {
        // TODO(re): other enemy types.
        let wanted = |k: &u16| {
            SUPPORTED.contains(k)
                || [GOOMBA_PIPE, crate::goal::GOAL, crate::autoscroll::AUTOSCROLL, crate::lifts::LIFT].contains(k)
        };
        Self { pending: list.iter().copied().filter(|e| wanted(&e.kind)).collect(), ..Self::default() }
    }

    /// The columns (left edge, px) whose entries spawn this frame: on the first frame
    /// everything from camera - 0x40 to camera + 0x110 (0x0804C440), then the one column at
    /// the edge the camera scrolls toward. Entries match their column exactly (x & 0xFFF0).
    fn columns(&mut self, camera_x: i32) -> Vec<i32> {
        let Some(last) = self.camera_x.replace(camera_x) else {
            let (first, end) = ((camera_x + START_FIRST) & !0xF, (camera_x + START_LAST) & !0xF);
            return (first..=end).step_by(16).map(|c| c + SPAWN_BEHIND).collect();
        };
        if camera_x != last {
            self.left = camera_x < last;
        }
        vec![(camera_x + if self.left { SPAWN_BEHIND } else { SPAWN_AHEAD }) & !0xF]
    }

    /// Moves list entries whose column came up into `active`, Goomba pipes into `pipes`
    /// (0x0804B460 shifts the eight generator slots and puts the new one first). Columns
    /// left of the level never spawn anything (the page test fails on them).
    pub fn spawn(&mut self, camera_x: i32, player: &Player, active: &mut Vec<Enemy>, pipes: &mut Vec<GoombaPipe>) {
        let columns = self.columns(camera_x);
        let mut goal = false;
        let mut autoscroll = None;
        let lifts = &mut self.lifts;
        self.pending.retain(|s| {
            let x = s.col as i32 * TILE_PX;
            if !columns.contains(&x) {
                return true;
            }
            match s.kind {
                GOOMBA_PIPE => {
                    pipes.insert(0, GoombaPipe::new(s));
                    pipes.truncate(GENERATOR_SLOTS);
                }
                crate::goal::GOAL => goal = true,
                crate::autoscroll::AUTOSCROLL => autoscroll = Some(s.row),
                crate::lifts::LIFT => lifts.push(*s),
                _ => active.push(Enemy::spawn(s, player)),
            }
            false
        });
        self.goal |= goal;
        self.autoscroll = self.autoscroll.or(autoscroll);
    }

    /// Lifts whose column came up.
    pub fn take_lifts(&mut self) -> Vec<EnemySpawn> {
        std::mem::take(&mut self.lifts)
    }

    /// An autoscroll entry's column came up (once): its row.
    pub fn take_autoscroll(&mut self) -> Option<u8> {
        self.autoscroll.take()
    }

    /// The goal card's column came up (once).
    pub fn take_goal(&mut self) -> bool {
        std::mem::take(&mut self.goal)
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
        projectile_touches(self.px(), player)
    }
}

/// 0x0808CC9C: a projectile at `(x, y)` px touches the player.
fn projectile_touches((x, y): (i32, i32), player: &Player) -> bool {
    let (fx, fy) = player.frame();
    let big = (player.form != crate::player::Form::Small) as usize;
    let dy = (y - fy - FIRE_TOP[big] + 8) as u16;
    let dx = (x - fx + 6) as u16;
    dy < FIRE_H[big] as u16 && dx < FIRE_W as u16
}

/// A Boomerang Bro's boomerang: projectile type 2 (thrown by 0x08068B04, handler
/// 0x0808D378). It sits in the Bro's hand for 15 frames (0x0808C9AC: +0x14 low nibble),
/// then flies: across at +/-0x20 for the first 0x40 frames of +0x15, then 1 a frame back
/// toward -/+0x20, holding 0x30 frames at each end (0x08170ADE / 0x08170ADC); up and down
/// every other frame by 1 between -0x16, 0x12 and back to 0 (0x08170AE0). Coming back it
/// is caught by the Bro (0x0808D2D4). It hurts like a fireball (0x0808CC9C).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Boomerang {
    /// Top-left, 1/256 px.
    pub x: i32,
    pub y: i32,
    /// Speeds in 1/16 px per frame (+0x0A / +0x08).
    pub vx: i32,
    vy: i32,
    /// +0x0C: direction step (bit 0: accelerate left), 0xFF once it no longer turns.
    dir: u8,
    /// +0x0D: vertical step.
    vert: u8,
    /// +0x13: spin counter.
    pub spin: u8,
    /// +0x15: frames before it may turn.
    hold: u8,
    /// Frames left in the Bro's hand (+0x14 & 0xF).
    pub held: u8,
    /// The thrower (`Enemy::id`).
    pub owner: u32,
    /// Projectile slot (0x0808C8FC: the highest free); the catch test runs on frames where
    /// slot + frame is even.
    pub slot: u8,
}

const BOOMERANG_VX: [i32; 2] = [0x20, -0x20];
const BOOMERANG_ACCEL: [i32; 2] = [1, -1];
const BOOMERANG_VY_END: [i32; 2] = [0x12, -0x12];
const BOOMERANG_VY: i32 = -0x16;
const BOOMERANG_HOLD: u8 = 0x40;
const BOOMERANG_TURN_HOLD: u8 = 0x30;
const BOOMERANG_IN_HAND: u8 = 0x0F;
/// In the hand (0x08170A90 / 0x08170A96, type 2): 8 px behind the facing, 3 px down.
const HAND_DX: i32 = 8;
const HAND_DY: i32 = 3;

impl Boomerang {
    /// 0x08068B04: at the Bro, speed 0x0816F042[side] (+0x20 toward a player on the right),
    /// direction 0x0816F044[side].
    fn thrown(bro: &Enemy, player: &Player) -> Self {
        let right = toward(player, bro.x) > 0;
        Self {
            x: bro.x,
            y: bro.y,
            vx: if right { 0x20 } else { -0x20 },
            vy: BOOMERANG_VY,
            dir: right as u8,
            vert: 0,
            spin: 0,
            hold: BOOMERANG_HOLD,
            held: BOOMERANG_IN_HAND,
            owner: bro.id,
            slot: PROJECTILE_SLOTS as u8 - 1,
        }
    }

    pub fn px(&self) -> (i32, i32) {
        (self.x >> 8, self.y >> 8)
    }

    /// Spin frame, a cell of `sma4_assets::boomerang_path` (flying left: the next four).
    pub fn cell(&self) -> usize {
        ((self.spin >> 2) & 3) as usize + if self.vx < 0 { sma4_assets::BOOMERANG_FRAMES as usize } else { 0 }
    }

    /// One frame (0x0808FB08 -> 0x0808D378). `owner` is the thrower if it is still alive.
    /// Returns false once it is gone (off the screen, caught, or its Bro is gone while it
    /// is held).
    pub fn step(&mut self, owner: Option<&Enemy>, player: &Player, camera: (i32, i32), frame: u32) -> bool {
        let (x, y) = self.px();
        if !(-8..0xF8).contains(&(x - camera.0)) || y - camera.1 + 0x14 >= 0x100 {
            return false;
        }
        self.hold = self.hold.saturating_sub(1);
        if self.held > 0 {
            // 0x0808C9AC: in the hand, the speed's sign following the Bro's facing.
            let Some(bro) = owner.filter(|b| b.state == EnemyState::Walking) else { return false };
            self.held -= 1;
            self.x = bro.x + if bro.facing_right { -HAND_DX } else { HAND_DX } * SUB;
            self.y = bro.y + HAND_DY * SUB;
            if (self.vx > 0) != bro.facing_right {
                self.vx = -self.vx;
                self.dir = self.dir.wrapping_add(1);
            }
            return true;
        }
        self.spin = self.spin.wrapping_add(1);
        let d = (self.dir & 1) as usize;
        if self.dir & 0x80 == 0 && self.hold == 0 {
            self.vx += BOOMERANG_ACCEL[d];
            if self.vx == BOOMERANG_VX[d] {
                self.hold = BOOMERANG_TURN_HOLD;
                self.dir = if self.vert != 0 { 0xFF } else { self.dir.wrapping_add(1) };
            }
        }
        if frame & 1 == 0 && (self.vert == 0 || self.vy != 0) {
            let v = (self.vert & 1) as usize;
            self.vy += BOOMERANG_ACCEL[v];
            if self.vy == BOOMERANG_VY_END[v] {
                self.vert += 1;
            }
        }
        self.x += self.vx * 16;
        self.y += self.vy * 16;
        // 0x0808D2D4: back home, the Bro catches it.
        if self.dir & 0x80 != 0
            && (self.slot as u32 + frame) & 1 == 0
            && let Some(bro) = owner.filter(|b| b.state == EnemyState::Walking)
        {
            let dy = ((self.y - (bro.y - 8 * SUB)) >> 8) - 8;
            let dx = (self.x - (bro.x - 8 * SUB)) >> 8;
            if (0..0x10).contains(&dy) && (0..0x10).contains(&dx) {
                return false;
            }
        }
        let _ = player;
        true
    }

    /// 0x0808CC9C, only once it has left the hand.
    pub fn touches(&self, player: &Player) -> bool {
        self.held == 0 && projectile_touches(self.px(), player)
    }
}

/// The highest projectile slot not in `used` (0x0808C8FC searches 5 down to 0).
pub fn free_slot(used: impl Iterator<Item = u8> + Clone) -> Option<u8> {
    (0..PROJECTILE_SLOTS as u8).rev().find(|s| !used.clone().any(|u| u == *s))
}

/// Projectile slots 0x03007848 + 0x8EC + i * 0x18 (six in all) shared by fireballs and
/// Micro-Goombas; 0x0808C8FC gives a Micro-Goomba the first free one from the top.
pub const PROJECTILE_SLOTS: usize = 6;

/// A Micro-Goomba dropped by the 0x74 Paragoomba: projectile type 6, handler 0x0808D95C.
/// It sways down to the ground; touching the player it sticks (0x0808CAEC: +0x14 = 5)
/// and rides along, holding the player's speed down, until enough button presses shake it
/// off; then it falls upside down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MicroGoomba {
    /// Top-left of its 8x8 OBJ, 1/256 px (+0 / +4).
    pub x: i32,
    pub y: i32,
    /// Speeds in 1/16 px per frame (+0x0A / +0x08).
    vx: i32,
    vy: i32,
    /// +0x14: 0 falling, 1-5 stuck to the player (presses left to shake it off), negative
    /// shaken off.
    pub state: i8,
    /// +0x13: sway phase while falling, the ride's wobble while stuck.
    count: u8,
}

/// Falling (0x0808D95C, state 0): down to 0x10 by 1 a frame, swaying by 1 a frame between
/// +/-0x10 (0x08170B02 / 0x08170B04). Shaken off (0x0808D910 / 0x0808D0E4): speed -0x20 up,
/// +/-8 across, then +2 a frame down to 0x6E.
const MICRO_FALL: i32 = 0x10;
const MICRO_SWAY: i32 = 0x10;
const MICRO_OFF_VY: i32 = -0x20;
const MICRO_OFF_VX: i32 = 8;
const MICRO_OFF_FALL: i32 = 0x6E;
/// Stuck: 0x0808CAEC sets 5; +1 every 16 frames (back to 5), -1 per press.
const MICRO_GRIP: i8 = 5;
/// Buttons whose press shakes it (0x03003AEC's low byte: A, B, Select, Start, the pad).
const MICRO_SHAKE: u16 = 0xFF;

impl MicroGoomba {
    /// 0x08054028: dropped at `(x, y)` (1/256 px), still.
    pub fn new(x: i32, y: i32) -> Self {
        Self { x, y, vx: 0, vy: 0, state: 0, count: 0 }
    }

    pub fn px(&self) -> (i32, i32) {
        (self.x >> 8, self.y >> 8)
    }

    pub fn stuck(&self) -> bool {
        self.state > 0
    }

    /// Cell of `sma4_assets::micro_goomba_path`: the OBJ flips every 16 frames (0x03003CF6
    /// bit 4); shaken off it falls upside down.
    pub fn cell(&self, frame: u32) -> usize {
        if self.state < 0 {
            sma4_assets::MICRO_GOOMBA_OFF
        } else if frame & 0x10 != 0 {
            sma4_assets::MICRO_GOOMBA_FLIP
        } else {
            0
        }
    }

    /// One frame (0x0808FB08 -> 0x0808D95C). `pressed` are this frame's new buttons;
    /// `can_stick` is false while the player cannot be hit (0x0808CAEC). Returns false once
    /// it has left the screen (x more than 8 px off either side, or more than 0xEC px
    /// below the view's top; above it is fine for this type).
    pub fn step(&mut self, player: &Player, camera: (i32, i32), frame: u32, pressed: u16, can_stick: bool) -> bool {
        let (x, y) = self.px();
        if !(-8..0xF8).contains(&(x - camera.0)) || y - camera.1 + 0x14 >= 0x100 {
            return false;
        }
        match self.state {
            0 => {
                self.x += self.vx * 16;
                self.y += self.vy * 16;
                if self.vy < MICRO_FALL {
                    self.vy += 1;
                }
                let right = self.count & 1 == 0;
                self.vx += if right { 1 } else { -1 };
                if self.vx == if right { MICRO_SWAY } else { -MICRO_SWAY } {
                    self.count = self.count.wrapping_add(1);
                }
                // The draw's touch test (0x0808CC9C -> 0x0808CAEC).
                if can_stick && projectile_touches(self.px(), player) {
                    self.state = MICRO_GRIP;
                }
            }
            s if s < 0 => {
                self.x += self.vx * 16;
                self.y += self.vy * 16;
                if self.vy < MICRO_OFF_FALL {
                    self.vy += 2;
                }
            }
            s => {
                if s < MICRO_GRIP && frame & 0xF == 0 {
                    self.state += 1;
                }
                if pressed & MICRO_SHAKE != 0 {
                    self.state -= 1;
                    if self.state == 0 {
                        self.shake_off();
                    }
                }
                self.ride(player);
            }
        }
        true
    }

    /// 0x0808D95C while stuck: wobbles over the player's frame, 0-15 px down (8-15 for a
    /// small player) and -3..12 px across, on a triangle of +0x13.
    fn ride(&mut self, player: &Player) {
        self.count = self.count.wrapping_add(1);
        let tri = |v: u8| if v > 15 { (v ^ 0xF) & 0xF } else { v } as i32;
        let mut dy = tri((self.count >> 2) & 0x1F);
        if player.form == crate::player::Form::Small {
            dy = (dy >> 1) + 8;
        }
        let dx = tri(self.count & 0x1F) - 3;
        let (fx, fy) = player.frame();
        self.x = (fx + dx) * SUB;
        self.y = (fy + dy) * SUB;
    }

    /// 0x0808D910.
    // TODO(re): the ROM picks the side from the random bits 0x03003879[slot] & 0x80.
    fn shake_off(&mut self) {
        self.state = -1;
        self.vy = MICRO_OFF_VY;
        self.vx = if self.count & 1 == 0 { MICRO_OFF_VX } else { -MICRO_OFF_VX };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::Buttons;

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
        EnemySpawn { kind, col, row: 6, ..Default::default() }
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
        Enemy::spawn(&EnemySpawn { kind: RED_PIRANHA, col: 10, row: 6, ..Default::default() }, &far_player())
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
        let mut e = Enemy::spawn(&EnemySpawn { kind: FIRE_PIRANHA, col: 10, row: 6, ..Default::default() }, &p);
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
        let mut e = Enemy::spawn(&EnemySpawn { kind: FIRE_PIRANHA, col: 10, row: 6, ..Default::default() }, &p);
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
    fn ceiling_plants_hang_out_below_their_pipe_then_go_up_into_it_even_with_the_player_near() {
        let lvl = level();
        let s = EnemySpawn { kind: CEILING_PIRANHA, col: 10, row: 3, ..Default::default() };
        // The player right under it: a ceiling plant has no shy test.
        let p = Player::spawn(10 * 16 + 8, 6 * 16);
        let mut e = Enemy::spawn(&s, &p);
        assert_eq!((e.px().1, e.home_y), (4 * 16, 4 * 16), "16 px below its row, at home");
        let (mut top, mut bottom) = (i32::MAX, i32::MIN);
        for f in 0..0x200 {
            e.step(&lvl, &p, f);
            top = top.min(e.px().1);
            bottom = bottom.max(e.px().1);
        }
        assert_eq!((top, bottom), (4 * 16 - RED_PLANT_REACH, 4 * 16));
    }

    #[test]
    fn buzzy_beetles_are_fire_proof_and_leave_a_shell_when_stomped() {
        let p = Player::spawn(0, 0);
        let mut e = Enemy::spawn(&enemy(BUZZY, 8), &p);
        let (x, y) = e.px();
        assert_eq!(e.fireball_hit((x + 4, y + 4), 1), Some(false), "the fireball bursts");
        assert_eq!(e.state, EnemyState::Walking);
        // Landing on it.
        let mut p = Player::spawn(x + 2, y - 12);
        p.vy = 16;
        assert_eq!(e.contact(&p), Contact::Stomped);
        assert!(matches!(e.state, EnemyState::Shell(_)));
        let mut g = Enemy::spawn(&enemy(WALKER, 8), &p);
        let (gx, gy) = g.px();
        assert_eq!(g.fireball_hit((gx + 4, gy + 4), 1), Some(true), "a Goomba is knocked out");
    }

    #[test]
    fn hidden_piranha_plant_ignores_the_tail() {
        let mut e = red_plant();
        let (ex, ey) = e.px();
        assert!(!e.tail_hit((ex, ey, 16, 16), &far_player()));
    }

    #[test]
    fn spawner_releases_supported_enemies_in_view_at_the_start_then_at_the_edge_once() {
        // Columns 5, 0x11 (x 0x110: the last one of the start), 0x12, 0x40; 0xA6 unsupported.
        let mut sp = Spawner::new(&[enemy(WALKER, 5), enemy(WALKER, 0x12), enemy(0xA6, 6), enemy(KOOPA, 0x11), enemy(WALKER, 0x40)]);
        let player = Player::spawn(0, 0);
        let mut active = Vec::new();
        sp.spawn(0, &player, &mut active, &mut Vec::new());
        assert_eq!(active.len(), 2);
        sp.spawn(0, &player, &mut active, &mut Vec::new());
        assert_eq!(active.len(), 2, "the camera has not moved");
        // Scrolling right: column 0x12 comes up at camera 0x10 (0x10 + 0x110 = 0x120).
        sp.spawn(0x0F, &player, &mut active, &mut Vec::new());
        assert_eq!(active.len(), 2);
        sp.spawn(0x10, &player, &mut active, &mut Vec::new());
        assert_eq!(active.len(), 3);
        sp.spawn(0x10, &player, &mut active, &mut Vec::new());
        assert_eq!(active.len(), 3, "once");
    }

    /// A player standing on the floor of `level()` with the left of its frame at `x`.
    fn player_at(x: i32) -> Player {
        let p0 = Player::spawn(0, 0);
        let (ox, oy) = (p0.px().0 - p0.frame().0, p0.px().1 - p0.frame().1);
        Player::spawn(x + ox, 7 * TILE_PX - 32 + oy)
    }

    #[test]
    fn goomba_climbs_over_a_hill_and_follows_the_slope_down() {
        let lvl = crate::level::test_hill();
        let p = player_at(40 * TILE_PX);
        let mut e = Enemy::spawn_at(WALKER, 2 * TILE_PX, 6 * TILE_PX, &p);
        let mut top = i32::MAX;
        for f in 0..900 {
            e.step(&lvl, &p, f);
            top = top.min(e.px().1);
            let (x, y) = e.px();
            // On the flat top (feet probe in columns 9-12) it stands on row 4.
            if (9 * TILE_PX..12 * TILE_PX).contains(&(x + 8)) {
                assert_eq!(y + SIZE, 4 * TILE_PX, "on the hilltop at x {x}");
            }
        }
        assert_eq!(top, 4 * TILE_PX - SIZE, "reached the top");
        assert!(e.px().0 > 16 * TILE_PX, "over the hill: {:?}", e.px());
        assert_eq!(e.px().1 + SIZE, 7 * TILE_PX, "back on the ground");
    }

    #[test]
    fn goomba_walks_slower_on_a_slope_moving_left() {
        let lvl = crate::level::test_hill();
        let p = player_at(0);
        // Walking left down the far side of the hill: the floor value is negative.
        let mut e = Enemy::spawn_at(WALKER, 16 * TILE_PX, 6 * TILE_PX, &p);
        let mut speeds = std::collections::BTreeSet::new();
        for f in 0..200 {
            e.step(&lvl, &p, f);
            speeds.insert(e.vx.abs());
        }
        assert_eq!(speeds.into_iter().collect::<Vec<_>>(), [SLOPE_WALK_SPEED, WALK_SPEED]);
    }

    #[test]
    fn micro_paragoomba_flies_above_the_player_and_drops_micro_goombas() {
        let lvl = level();
        let p = player_at(4 * TILE_PX);
        let (_, fy) = p.frame();
        let mut e = Enemy::spawn_at(MICRO_PARAGOOMBA, 8 * TILE_PX, 6 * TILE_PX, &p);
        let mut drops = Vec::new();
        let mut highest = i32::MAX;
        let mut landed_again = None;
        for f in 0..400 {
            e.step(&lvl, &p, f);
            highest = highest.min(e.px().1);
            if let Some(m) = e.dropped {
                // Dropped from where the body was before this frame's move.
                let (dx, dy) = (m.px().0 - e.px().0, m.px().1 - e.px().1);
                assert!((3..=5).contains(&dx) && (3..=5).contains(&dy), "{:?} {:?}", m.px(), e.px());
                drops.push(f);
            }
            if landed_again.is_none() && !drops.is_empty() && e.px().1 == 6 * TILE_PX {
                landed_again = Some(f);
            }
        }
        // Up until 0x38 px above the player's frame, then it slows to a stop (16 frames).
        assert!(fy - highest > MICRO_PARA_HEIGHT as i32, "rose to {highest}, player at {fy}");
        // Hovering 0x80 frames: a Micro-Goomba every 0x20 frames.
        assert!(drops[..4].windows(2).all(|w| w[1] - w[0] == 0x20), "{drops:?}");
        // Then it comes back down, walks out the 0x80 timer and flies off again.
        let landed = landed_again.expect("comes back down");
        assert!(drops[3] < landed && drops[4] > landed, "{drops:?}, landed at {landed}");
    }

    #[test]
    fn stomping_a_micro_paragoomba_leaves_a_goomba() {
        let lvl = level();
        let p = far_player();
        let mut e = Enemy::spawn_at(MICRO_PARAGOOMBA, 8 * TILE_PX, 6 * TILE_PX, &p);
        e.step(&lvl, &p, 0);
        let (ex, ey) = e.px();
        let jumper = Player::spawn(ex, ey + 4 - 16);
        assert_eq!(e.contact(&jumper), Contact::Stomped, "{e:?}");
        assert_eq!(e.kind, WALKER);
        assert!(!e.was_para, "brown like any Goomba");
    }

    #[test]
    fn micro_goomba_sways_down_sticks_to_the_player_and_is_shaken_off() {
        let p = player_at(100);
        let (fx, fy) = p.frame();
        let mut m = MicroGoomba::new((fx + 4) * SUB, (fy - 40) * SUB);
        let camera = (0, 0);
        let mut xs = Vec::new();
        let mut f = 0;
        while !m.stuck() {
            assert!(m.step(&p, camera, f, 0, true));
            xs.push(m.px().0);
            f += 1;
            assert!(f < 200, "never reached the player");
        }
        // It swayed both ways on the way down.
        assert!(xs.iter().max() > xs.iter().min());
        assert_eq!(m.state, MICRO_GRIP);
        // Riding: it stays over the player's frame.
        for _ in 0..40 {
            m.step(&p, camera, f, 0, true);
            let (x, y) = m.px();
            assert!((fx - 3..=fx + 12).contains(&x) && (fy..=fy + 15).contains(&y), "{:?}", m.px());
            f += 1;
        }
        // Five presses, quicker than the grip comes back (+1 every 16 frames).
        for _ in 0..5 {
            m.step(&p, camera, f, Buttons::A.bits(), true);
            f += 1;
        }
        assert!(m.state < 0, "shaken off: {}", m.state);
        let y = m.px().1;
        for _ in 0..60 {
            m.step(&p, camera, f, 0, true);
            f += 1;
        }
        assert!(m.px().1 > y, "falls away");
    }

    #[test]
    fn micro_goomba_does_not_stick_while_the_player_cannot_be_hit() {
        let p = player_at(100);
        let (fx, fy) = p.frame();
        let mut m = MicroGoomba::new((fx + 4) * SUB, (fy + 12) * SUB);
        for f in 0..10 {
            m.step(&p, (0, 0), f, 0, false);
        }
        assert!(!m.stuck());
    }

    #[test]
    fn goomba_pipe_sends_goombas_out_three_times_in_four_while_the_player_is_right() {
        let pipe = EnemySpawn { kind: GOOMBA_PIPE, col: 4, row: 5, ..Default::default() };
        let mut g = GoombaPipe::new(&pipe);
        let right = player_at(12 * TILE_PX);
        let mut out = Vec::new();
        for f in 0..(0x60 + 4 * 0x70 - 1) {
            if let Some(e) = g.step(&right, true) {
                assert_eq!(e.px(), (4 * TILE_PX, 5 * TILE_PX - 3));
                assert!(e.from_pipe && e.emerging == PIPE_EMERGE && e.vx > 0);
                out.push(f + 1);
            }
        }
        // First after 0x60 frames, then every 0x70; the fourth try is skipped.
        assert_eq!(out, [0x60, 0x60 + 0x70, 0x60 + 2 * 0x70]);
        // With the player on the pipe's left, or no free slot, nothing comes out.
        let left = player_at(0);
        let mut g = GoombaPipe::new(&pipe);
        assert!((0..0x200).all(|_| g.step(&left, true).is_none()));
        let mut g = GoombaPipe::new(&pipe);
        assert!((0..0x200).all(|_| g.step(&right, false).is_none()));
    }

    #[test]
    fn a_goomba_from_a_pipe_slides_out_for_0x28_frames_then_falls_and_walks() {
        let lvl = level();
        let p = player_at(20 * TILE_PX);
        let mut g = GoombaPipe::new(&EnemySpawn { kind: GOOMBA_PIPE, col: 4, row: 4, ..Default::default() });
        let mut e = (0..0x100).find_map(|_| g.step(&p, true)).unwrap();
        let (x0, y0) = e.px();
        for f in 0..PIPE_EMERGE as u32 {
            e.step(&lvl, &p, f);
            assert_eq!(e.px().1, y0, "no gravity while it comes out");
        }
        assert_eq!(e.emerging, 0);
        assert_eq!(e.px().0, x0 + PIPE_EMERGE as i32 / 2, "half a pixel a frame");
        for f in 0..60 {
            e.step(&lvl, &p, f);
        }
        assert_eq!(e.px().1, 6 * TILE_PX, "landed on the floor");
        assert!(e.off_screen((e.px().0 - 0x121, 0)) && e.off_screen((e.px().0 + 0x41, 0)));
        assert!(!e.off_screen((e.px().0 - 0x120, 0)) && !e.off_screen((e.px().0 + 0x40, 0)));
    }

    #[test]
    fn boomerang_bro_paces_throws_and_catches_its_boomerang() {
        let lvl = level();
        let p = player_at(2 * TILE_PX);
        let mut e = Enemy::spawn_at(BOOMERANG_BRO, 10 * TILE_PX, 4 * TILE_PX, &p);
        e.init_random(0);
        let mut xs = Vec::new();
        let mut boomerangs: Vec<Boomerang> = Vec::new();
        let (mut thrown, mut caught) = (0, 0);
        for f in 1..600 {
            e.step(&lvl, &p, f);
            xs.push(e.px().0);
            if let Some(mut b) = e.thrown.take() {
                b.slot = free_slot(boomerangs.iter().map(|b| b.slot)).unwrap();
                assert!(b.vx < 0, "thrown toward the player on the left");
                boomerangs.push(b);
                thrown += 1;
            }
            let before = boomerangs.len();
            boomerangs.retain_mut(|b| b.step(Some(&e), &p, (0, 0), f));
            caught += before - boomerangs.len();
        }
        assert_eq!(e.px().1 + BRO_HEIGHT, 7 * TILE_PX, "stands on the floor");
        // It paces: never more than a walk (0x40 frames at half a pixel) from home.
        let (lo, hi) = (*xs.iter().min().unwrap(), *xs.iter().max().unwrap());
        assert!(hi - lo <= 33 && hi > lo, "paces {lo}..{hi}");
        assert!(thrown >= 4, "throws twice per walk cycle: {thrown}");
        assert!(caught >= 2, "boomerangs come back: {caught}");
    }

    #[test]
    fn stomping_a_boomerang_bro_knocks_it_out_for_a_bigger_score() {
        let lvl = level();
        let p = far_player();
        let mut e = Enemy::spawn_at(BOOMERANG_BRO, 8 * TILE_PX, 4 * TILE_PX, &p);
        for f in 0..30 {
            e.step(&lvl, &p, f);
        }
        let (ex, ey) = e.px();
        let jumper = Player::spawn(ex, ey + 4 - 16);
        assert_eq!(e.contact(&jumper), Contact::Stomped);
        assert_eq!(e.state, EnemyState::Dead);
        assert_eq!(e.stomp_bonus(), BRO_STOMP_BONUS);
    }
}
