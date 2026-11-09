//! Larry Koopa, the boss of World 1's airship (object type 0x0E in his cabin, level 0x213;
//! init 0x08071C60, main 0x080735B4 -> 0x0816F89C[0x030038C6]) and his magic rings
//! (projectile type 8, handler 0x0808DC68). See docs/re-notes.md "Larry Koopa".
//!
//! * State 0 (0x080730F8 -> 0x08072D60 outside World 6): drawn first (0x08072954), then
//!   he moves (0x0807244C: x and y by the speed bytes, the probes of class 0x30, + 2 a frame
//!   with no cap, landing, a ceiling sends him down at 8, a wall reverses his speed
//!   but not his facing), ten fireballs
//!   beat him (0x08072248), the player's touch (0x08048AC4 -> 0x080735DC). On the ground
//!   he walks toward the player (0x10, re-aimed every 8 frames), jumps when the frame
//!   counter & 0x0816F85E[hits] is 0 (speed 0x0816F873[hits]) and otherwise casts when it
//!   & 0x0816F849[hits] is 0: the aim 0x0807254C, then +0x23 = 0x20 and a ring each time
//!   it reaches 0x18, 0x10 and 8 (0x08072268).
//! * A stomp (0x080735DC): the player bounces (0xD0), sound 0x7E, and 0x0807214C: speeds
//!   stop, one hit more (+0x3B); below his three hit points (+0x1E) he spins in his shell
//!   for 0x80 frames (+0x24): 0x40 still, then hops toward the player (0x10, 0xB8). In the
//!   shell (frames 4 and up) a touch from 4-15 px above pushes the player aside and bounces
//!   him; from further above or below nothing happens.
//! * The third hit: the clock stops (0x03003D0F = 0x81), state 1 (0x08073124): he lands,
//!   takes speed (0x80 - x) / 2 toward the middle, spins 0xA0 frames, then flies up (0xA0)
//!   until both his rows are above the screen with x below 0x88: state 2 and the wand
//!   falls (`crate::wand`, 0x0807230C).
//!
//! Positions in 1/256 px, speeds in 1/16 px per frame (the ROM's speed bytes).

use crate::level::{Level, TILE_PX};
use crate::obj::ObjSprite;
use crate::player::{Player, SUB, UNIT};
use sma4_assets::TileKind;

pub const LARRY: u16 = 0x0E;

/// Init 0x08071C60: three hit points (+0x1E), ten fireballs (+0x36).
const HIT_POINTS: u8 = 3;
const FIRE_HITS: u8 = 10;
/// Probes for class 0x30 (0x0816A2BB[0x0E] & 0xF8, 0x08169E78[0x30..]) as (dy, dx).
const FEET: (i32, i32) = (0x20, 0x0C);
const HEAD: (i32, i32) = (0x00, 0x0C);
const WALL_LEFT: (i32, i32) = (0x11, 0x01);
const WALL_RIGHT: (i32, i32) = (0x11, 0x17);
/// 0x0807244C: + 2 a frame (no cap); a ceiling sends him down at 8.
const GRAVITY: i32 = 2;
const CEILING_SPEED: i32 = 8;
/// State 1's fall (0x08049EC0): + 3 up to 0x40 (0x08169D9E / 0x08169D9C), landing only
/// within the tile's top 6 px (0x08045E00).
const FALL_GRAVITY: i32 = 3;
const FALL_MAX: i32 = 0x40;
const FALL_LAND_DEPTH: i32 = 6;
const WALK: i32 = 0x10;
/// By hits so far (0x0816F846[hits] = 0, 7, 14, World 1): casting mask 0x0816F849, jump
/// mask 0x0816F85E and jump speed 0x0816F873.
const MAGIC_MASK: [u8; 3] = [0x63, 0x63, 0x33];
const JUMP_MASK: [u8; 3] = [0xBF, 0xBF, 0xDF];
const JUMP_SPEED: [i32; 3] = [-0x30, -0x40, -0x50];
const MAGIC_TIME: u8 = 0x20;
/// Hopping in the shell (0x0816F844 toward the player, speed 0xB8).
const HOP_VX: i32 = 0x10;
const HOP_VY: i32 = -0x48;
/// +0x24 after a hit, and after the last one.
const SHELL_TIME: u8 = 0x80;
const BEATEN_TIME: u8 = 0xA0;
/// Fireballs left after the last hit (+0x36 = 0x7F).
const BEATEN_FIRE: u8 = 0x7F;
/// The player's bounce off him (+0x1D = 0xD0) and the push of the shell (+0x1C 0xD0 / 0x30).
pub const STOMP_BOUNCE: i32 = -0x30 * UNIT;
pub const SHELL_PUSH: i32 = 0x30 * UNIT;
/// State 1: the speed toward x 0x80 is (0x80 - x low byte) >> 1; up at 0xA0 once +0x24 is
/// below 8; the wand comes once both rows are off the top (+0x1B == 3) with (x low byte -
/// 0x78) < 0x10.
const MIDDLE: i32 = 0x80;
const FLY_UP: i32 = -0x60;
const WAND_X: i32 = 0x78;
/// Spin frames by state (0x08072954): + 1 every 4 frames of +0x24 up to 0x0816F633[state]
/// (exclusive), then back to 0x0816F635[state]; the flags +0x1A by frame (0x0816F637).
const SPIN_END: [u8; 2] = [0x0A, 0x12];
const SPIN_LOOP: [u8; 2] = [0x04, 0x0A];
const SPIN_FLAGS: [u8; 18] = [0, 0x40, 0, 0x40, 0, 0, 0x40, 0x40, 0, 0, 0, 0, 0, 0x40, 0x40, 0x40, 0, 0];
/// Body tiles (0x0816F650): six 8x16 OBJs, two rows of three, by frame - 4 (0xFFFF: none).
/// Frames 0-3 use rows 0x12.. (0x0816F649[world] + frame, frame 3 as 0).
const BODY: [[u16; 6]; 18] = [
    [0x15C, 0xFFFF, 0x15C, 0x150, 0x151, 0x152],
    [0x15D, 0xFFFF, 0x15D, 0x156, 0x157, 0x158],
    [0x15E, 0xFFFF, 0x15E, 0x150, 0x151, 0x152],
    [0x15C, 0xFFFF, 0x15C, 0x153, 0x154, 0x155],
    [0x15D, 0xFFFF, 0x15D, 0x159, 0x15A, 0x15B],
    [0x15E, 0xFFFF, 0x15E, 0x153, 0x154, 0x155],
    [0x116, 0x117, 0xFFFF, 0x118, 0x119, 0xFFFF],
    [0x112, 0x113, 0xFFFF, 0x114, 0x115, 0xFFFF],
    [0x110, 0x110, 0xFFFF, 0x111, 0x111, 0xFFFF],
    [0xFFFF, 0x112, 0x113, 0xFFFF, 0x114, 0x115],
    [0xFFFF, 0x116, 0x117, 0xFFFF, 0x118, 0x119],
    [0xFFFF, 0x11A, 0x11B, 0xFFFF, 0x11C, 0x11D],
    [0x11E, 0x11E, 0xFFFF, 0x11F, 0x11F, 0xFFFF],
    [0x11A, 0x11B, 0xFFFF, 0x11C, 0x11D, 0xFFFF],
    [0x180, 0x181, 0x182, 0x183, 0x184, 0x185],
    [0x180, 0x181, 0x182, 0x183, 0x186, 0x187],
    [0x180, 0x188, 0x182, 0x189, 0x18A, 0x185],
    [0x1C0, 0x1C1, 0x1C2, 0x1C3, 0x1C4, 0x1C5],
];
const STAND_ROWS: usize = 0x12;
pub const PALETTE: u8 = 12;
/// Palette 12 while a fireball's flash shows (0x0821F300 + 0x0816F830[world] * 2; the
/// host loads it as `sma4_assets::larry_flash_path`).
pub const FLASH_PALETTE: u8 = 16;
/// The wand in his hand (0x08072680, frames 0-3): offsets 0x0816F804 (y) / 0x0816F810 (x,
/// facing right + 0xC) at 0x0816F828[world] + frame, tiles 0x0816F7F4 + 0x0816F800[frame].
const WAND_DY: [i32; 4] = [0x0A, 0x0A, -0x01, 0x0F];
const WAND_DX: [[i32; 4]; 2] = [[-0x06, -0x06, 0x0A, -0x08], [0x0E, 0x0E, -0x02, 0x10]];
const WAND_TILES: [[u16; 2]; 4] = [[0x99, 0x98], [0x99, 0x98], [0x98, 0x99], [0x9B, 0x9A]];
/// The on-screen tests 0x08048138 (columns 0x08169616 / bits 0x0816961C, 0x0816A18E[0x0E]
/// = 0x25: three columns) and 0x080481B4 (two rows).
const COLUMNS: [(i32, u8); 3] = [(0x00, 0x80), (0x08, 0x40), (0x10, 0x20)];
const ROWS: usize = 2;
/// His contact box 0x08169F44[+0x64 & 0x3F = 0x32]: (dx, w, dy, h); the player's boxes
/// (0x0804865C, 0x08169624): small or crouching, and big.
const BOX: (i32, i32, i32, i32) = (6, 0x14, 4, 0x1A);
/// Beaten and spinning (state 1, frames 0x0A..): +0x64 = 0x41E, box 0x1E.
const BEATEN_BOX: (i32, i32, i32, i32) = (2, 0x0C, 0x10, 0x10);
const PLAYER_BOX: [(i32, i32, i32, i32); 2] = [(4, 8, 0x11, 0x0D), (3, 0x0A, 5, 0x19)];
/// 0x08048960: a touch from above no deeper than this is a stomp.
const STOMP_DEPTH: i32 = 9;

/// Rings (projectile 8, 0x08072268): from x - 14 (facing left) or + 30 (right), y + 0x0A,
/// in the first free of slots 5..0, timer +0x15 from 0xFF (tiles 0x96, 0xD8 below 0xE0,
/// 0xD9 below 0xC0).
const RING_DX: [i32; 2] = [-14, 30];
const RING_DY: i32 = 0x0A;
pub const RING_SLOTS: usize = 6;
const RING_PALETTE: u8 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ring {
    pub x: i32,
    pub y: i32,
    /// +0x0A / +0x08.
    pub vx: i32,
    pub vy: i32,
    /// +0x15.
    pub timer: u8,
}

impl Ring {
    pub fn px(&self) -> (i32, i32) {
        (self.x >> 8, self.y >> 8)
    }

    /// 0x0808CC9C: (y - frame y - 0x08170A9C[big] + 8) as u16 < 0x08170A9E[big], (x -
    /// frame x + 6) as u16 < 0x10.
    fn hits(&self, (fx, fy): (i32, i32), big: bool) -> bool {
        const TOP: [i32; 2] = [0x12, 0x0A];
        const H: [i32; 2] = [0x10, 0x16];
        let (x, y) = self.px();
        ((y - fy - TOP[big as usize] + 8) as u16) < H[big as usize] as u16 && ((x - fx + 6) as u16) < 0x10
    }

    fn tile(&self) -> u16 {
        match self.timer {
            0xE0.. => 0x96,
            0xC0.. => 0xD8,
            _ => 0xD9,
        }
    }
}

/// What Larry did this frame; the game applies it.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Events {
    pub hurt_player: bool,
    /// The player bounces off him (+0x1D = 0xD0).
    pub bounce: bool,
    /// The shell pushes the player aside: his new x speed (+0x1C), in the port's units.
    pub push: Option<i32>,
    /// A stomp counted (sound 0x7E).
    pub stomped: bool,
    /// The last hit: the clock stops (0x03003D0F = 0x81).
    pub beaten: bool,
    /// State 2: the wand appears (0x0807230C).
    pub wand: bool,
}

/// What Larry needs of the player each frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerView {
    /// The ROM's +0 / +4: the frame's top-left, 1/256 px.
    pub x: i32,
    pub y: i32,
    /// +0x1D, the speed byte (1/16 px).
    pub vy: i32,
    /// +0x35 == 0.
    pub on_ground: bool,
    /// 0x0804865C's box: small (or crouching) or big.
    pub small: bool,
    /// Contact is off (dying, behind the scenery: +0x62 / +0x55).
    pub ignore: bool,
}

impl PlayerView {
    pub fn of(p: &Player) -> Self {
        let (fx, fy) = p.frame();
        Self {
            x: fx * SUB + p.x.rem_euclid(SUB),
            y: fy * SUB + p.y.rem_euclid(SUB),
            vy: p.vy.div_euclid(UNIT),
            on_ground: p.on_ground,
            small: p.small_box(),
            ignore: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Larry {
    pub x: i32,
    pub y: i32,
    /// +0x10 / +0x12, speed bytes.
    pub vx: i32,
    pub vy: i32,
    /// +0x13 probe bits: 1 right wall, 2 left wall, 4 ground, 8 ceiling.
    pub bits: u8,
    /// +0x14 / +0x1B off-screen bits (columns / rows).
    pub off_h: u8,
    pub off_v: u8,
    /// +0x17.
    pub frame: u8,
    /// +0x1A: bit 6 facing right.
    pub flags: u8,
    /// +0x23 casting, +0x24 shell, +0x25 fireball flash.
    pub magic: u8,
    pub shell: u8,
    pub flash: u8,
    /// +0x36 fireballs left, +0x3B hits, +0x1E hit points.
    pub fire: u8,
    pub hits: u8,
    hp: u8,
    /// +0x3E / +0x3F: the aim (x, y speeds of the next rings).
    pub aim: (i32, i32),
    /// +0x44: state 1 has landed.
    landed: bool,
    /// +0x64 = 0x41E (state 1): the smaller box.
    beaten_box: bool,
    /// 0x030038C6: 0 fighting, 1 beaten, 2 the wand fell.
    pub state: u8,
    /// Palette 12 holds the flash colours (0x08072954 writes it on +0x25 & 3 == 0 / 1).
    pub flash_palette: bool,
    pub rings: [Option<Ring>; RING_SLOTS],
    /// +0xC / +0xE: the screen position the draw left (the contact uses it).
    screen: (i32, i32),
    /// His OAM entries from this frame's draw: the wand in his hand, then the body.
    sprites: Vec<ObjSprite>,
}

impl Larry {
    /// The list cell's top-left (px).
    pub fn spawn(x: i32, y: i32) -> Self {
        Self {
            x: x * SUB,
            y: y * SUB,
            vx: 0,
            vy: 0,
            bits: 0,
            off_h: 0,
            off_v: 0,
            frame: 0,
            flags: 0,
            magic: 0,
            shell: 0,
            flash: 0,
            fire: FIRE_HITS,
            hits: 0,
            hp: HIT_POINTS,
            aim: (0, 0),
            landed: false,
            beaten_box: false,
            state: 0,
            flash_palette: false,
            rings: [None; RING_SLOTS],
            screen: (0, 0),
            sprites: Vec::new(),
        }
    }

    pub fn px(&self) -> (i32, i32) {
        (self.x >> 8, self.y >> 8)
    }

    fn facing_right(&self) -> bool {
        self.flags & 0x40 != 0
    }

    fn on_ground(&self) -> bool {
        self.bits & 4 != 0
    }

    /// 0x08048138 (param 2) and 0x080481B4 (param 1) at (x, y).
    fn off_screen(x: i32, y: i32, camera: (i32, i32)) -> (u8, u8) {
        let mut h = 0;
        for (dx, bit) in COLUMNS {
            if ((dx + (x >> 8) - camera.0) & 0xFFFF) > 0xEF {
                h |= bit;
            }
        }
        let mut v = 0;
        let mut row = ((y >> 8) - camera.1 + 0x20) & 0xFFFF;
        for k in 0..ROWS {
            row = (row + 0x10) & 0xFFFF;
            if row > 0xEF {
                v |= 1 << k;
            }
        }
        (h, v)
    }

    fn solid(level: &Level, x: i32, y: i32) -> bool {
        level.at(x.div_euclid(TILE_PX), y.div_euclid(TILE_PX)) == TileKind::Solid
    }

    /// 0x08045ABC's probes after a move: the wall on the side he moves to (standing, odd
    /// frames right, even left), then the feet (not rising) or the head.
    fn probe(&mut self, level: &Level, counter: u8, land_depth: i32) {
        let (px, py) = self.px();
        self.bits = 0;
        let right = self.vx > 0 || (self.vx == 0 && counter & 1 == 1);
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
            // 0x080453E8: within the tile's top `land_depth` px.
            if k != TileKind::Empty && (py & 0xF) < land_depth {
                self.bits |= 4;
            }
        }
    }

    /// 0x08045384: the feet onto the tile's top, speed 0.
    fn land(&mut self) {
        self.y &= !0xFFF;
        self.vy = 0;
    }

    /// 0x0807244C (World 1).
    fn physics(&mut self, level: &Level, counter: u8) {
        self.x += self.vx * 16;
        self.y += self.vy * 16;
        self.probe(level, counter, 0x10);
        self.vy = (self.vy + GRAVITY) as i8 as i32;
        if self.on_ground() {
            self.land();
        }
        if self.bits & 8 != 0 {
            self.vy = CEILING_SPEED;
        }
        if self.bits & 3 != 0 {
            // 0x0804A028 turns speed and facing, then 0x0804A01C turns the facing back.
            self.vx = -self.vx;
        }
    }

    /// 0x08049F48: the player is left of him (the pixel difference's sign).
    fn player_left(&self, p: &PlayerView) -> bool {
        (((p.x - self.x) >> 8) as i16) < 0
    }

    /// 0x0807254C: the rings' speeds toward the player (whose pixel row is taken as at
    /// least 0x50 within its page): 0x14 along the longer axis, the other counted over 21
    /// steps of an 8-bit accumulator that wraps (as the ROM's does).
    fn aim_at(&mut self, p: &PlayerView) {
        let mut py = p.y;
        if (py >> 8) & 0xFF < 0x50 {
            py = (py & !0xFF00) | 0x5000;
        }
        let dy = ((py - self.y) >> 8) as i16;
        let dx = ((p.x - self.x) >> 8) as i16;
        let ay = (dy as i8).unsigned_abs();
        let ax = (dx as i8).unsigned_abs();
        let y_major = (ax as i8) < (ay as i8);
        let (mut major, minor) = if y_major { (ay, ax) } else { (ax, ay) };
        let mut acc = 0u8;
        let mut count = 0i32;
        for _ in 0..21 {
            let s = (acc as i8 as i32 + minor as i8 as i32) as u8;
            if (major as i8) <= (s as i8) {
                major = (major as i8 as i32 - 1) as u8;
                count += 1;
            }
            acc = s;
        }
        let (mut vy, mut vx) = if y_major { (0x14, count) } else { (count, 0x14) };
        if dy < 0 {
            vy = -vy;
        }
        if dx < 0 {
            vx = -vx;
        }
        self.aim = (vx as i8 as i32, vy as i8 as i32);
    }

    /// 0x08072268: a ring in the first free slot of 5..0, unless he is off the screen.
    fn cast(&mut self) {
        if self.off_h | self.off_v != 0 {
            return;
        }
        let Some(slot) = (0..RING_SLOTS).rev().find(|&i| self.rings[i].is_none()) else { return };
        self.rings[slot] = Some(Ring {
            x: self.x + RING_DX[self.facing_right() as usize] * SUB,
            y: self.y + RING_DY * SUB,
            vx: self.aim.0,
            vy: self.aim.1,
            timer: 0xFF,
        });
    }

    /// 0x0807214C: a stomp (or the last fireball) counts.
    fn hit(&mut self, ev: &mut Events) {
        self.vx = 0;
        if self.vy < 0 {
            self.vy = 0;
        }
        self.hits += 1;
        if self.hits < self.hp {
            self.shell = SHELL_TIME;
            return;
        }
        // Every other object (slots 4..0) would become a puff; none are in the cabin.
        self.state = 1;
        self.fire = BEATEN_FIRE;
        self.shell = BEATEN_TIME;
        self.flash_palette = false;
        ev.beaten = true;
    }

    /// 0x08048960 (0x080484F4 / 0x0804865C boxes, 0x08048718) -> 0x080735DC.
    fn contact(&mut self, p: &PlayerView, camera: (i32, i32), counter: u8, ev: &mut Events) {
        if p.ignore {
            return;
        }
        // Screen positions as the draw left them (+0xC / +0xE, before the move).
        let (sx, sy) = self.screen;
        let (ox, ow, oy, oh) = ((BOX.0 + sx) as u8, BOX.1 as u8, (BOX.2 + sy) as u8, BOX.3 as u8);
        let pb = PLAYER_BOX[!p.small as usize];
        let (fx, fy) = ((p.x >> 8) - camera.0, (p.y >> 8) - camera.1);
        let (qx, qw, qy, qh) = ((pb.0 + fx) as u8, pb.1 as u8, (pb.2 + fy) as u8, pb.3 as u8);
        let d = qx.wrapping_sub(ox);
        let (d, ext) = if (d as i8) < 0 { (d.wrapping_neg(), qw) } else { (d, ow) };
        if d > ext {
            return;
        }
        let d = qy.wrapping_sub(oy);
        let (d, ext, mut above) = if (d as i8) < 0 { (d.wrapping_neg(), qh, true) } else { (d, oh, false) };
        if d > ext {
            return;
        }
        if ((p.x - self.x).abs() >> 8) > 0x7F || ((p.y - self.y).abs() >> 8) >= 0x80 {
            return;
        }
        if above && ext as i32 - d as i32 > STOMP_DEPTH {
            above = false;
        }
        // 0x080735DC.
        let mut stomp = above;
        if self.frame > 3 {
            // 0x0804A3D8: (his y - the player's) in 4 px steps.
            let steps = ((self.y - p.y) >> 10) as u8;
            if steps > 3 {
                return;
            }
            if steps != 0 {
                ev.push = Some(if counter & 1 == 0 { -SHELL_PUSH } else { SHELL_PUSH });
                stomp = true;
            }
        }
        if !stomp {
            ev.hurt_player = true;
            return;
        }
        if p.on_ground || p.vy > 7 {
            ev.bounce = true;
            if self.shell == 0 {
                ev.stomped = true;
                self.hit(ev);
            }
        }
    }

    /// One frame of the object loop and main 0x080735B4. `counter` is 0x03003AE8, `held`
    /// 0x03007848 + 0xBD1 (everything waits).
    pub fn step(&mut self, level: &Level, p: &PlayerView, camera: (i32, i32), counter: u8, held: bool) -> Events {
        let mut ev = Events::default();
        // The object loop: timers down, the on-screen bits from where he is.
        if !held {
            self.magic = self.magic.saturating_sub(1);
            self.shell = self.shell.saturating_sub(1);
            self.flash = self.flash.saturating_sub(1);
        }
        (self.off_h, self.off_v) = Self::off_screen(self.x, self.y, camera);
        match self.state {
            0 => self.fight(level, p, camera, counter, held, &mut ev),
            1 => self.beaten(level, camera, counter, held, &mut ev),
            _ => self.sprites.clear(),
        }
        if !held {
            self.step_rings(camera);
        }
        ev
    }

    /// 0x08072D60.
    fn fight(&mut self, level: &Level, p: &PlayerView, camera: (i32, i32), counter: u8, held: bool, ev: &mut Events) {
        self.draw(camera, held);
        if held {
            return;
        }
        self.physics(level, counter);
        // 0x08072248: the tenth fireball.
        if self.fire == 0 {
            self.hits = self.hp - 1;
            self.hit(ev);
        }
        self.contact(p, camera, counter, ev);
        // 0x0804A3AC: toward the player.
        let toward = if ((self.x - p.x) >> 8) < 1 { 0x40 } else { 0 };
        if self.shell != 0 {
            if self.shell > 0x3F {
                return;
            }
            if self.shell == 1 {
                self.frame = 0;
                self.flags = toward;
                return;
            }
            if self.on_ground() {
                self.vx = if self.player_left(p) { -HOP_VX } else { HOP_VX };
                self.vy = HOP_VY;
            }
            return;
        }
        if self.magic != 0 {
            if self.magic & 7 == 0 {
                self.frame = 3;
                self.cast();
            }
            return;
        }
        if !self.on_ground() {
            return;
        }
        if counter & 7 == 0 {
            self.flags = toward;
            self.vx = if toward != 0 { WALK } else { -WALK };
        }
        self.frame = (counter >> 3) & 1;
        let i = (self.hits as usize).min(2);
        if JUMP_MASK[i] & counter == 0 {
            self.vy = JUMP_SPEED[i];
            return;
        }
        if MAGIC_MASK[i] & counter != 0 {
            return;
        }
        self.flags = toward;
        self.frame = 2;
        self.vx = 0;
        self.aim_at(p);
        self.magic = MAGIC_TIME;
    }

    /// 0x08073124.
    fn beaten(&mut self, level: &Level, camera: (i32, i32), counter: u8, held: bool, ev: &mut Events) {
        self.draw(camera, held);
        if !self.landed {
            // 0x08049EC0.
            self.x += self.vx * 16;
            self.y += self.vy * 16;
            self.probe(level, counter, FALL_LAND_DEPTH);
            self.vy = (self.vy + FALL_GRAVITY).min(FALL_MAX);
            if self.on_ground() {
                self.land();
                self.vx = ((MIDDLE - (self.px().0 & 0xFF)) >> 1) as i8 as i32;
                self.landed = true;
            }
            return;
        }
        if held {
            return;
        }
        if self.shell == 8 {
            // Sound 0x17.
            return;
        }
        if self.frame > 9 {
            self.beaten_box = true;
        }
        if self.shell < 8 {
            self.shell = 4;
            if self.off_v == 3 && (self.px().0 & 0xFF) - WAND_X < 0x10 {
                self.state = 2;
                ev.wand = true;
                return;
            }
            self.x += self.vx * 16;
            self.vy = FLY_UP;
            self.y += self.vy * 16;
        }
    }

    /// A player fireball at (x, y) px (0x0808B2F0 with box 0x32, after the objects): true
    /// if it hit him (it bursts); one less of +0x36 and the flash (0x0816A18E[0x0E] bit 0).
    /// Beaten (state 1) he still takes them, to no effect.
    pub fn fireball(&mut self, (x, y): (i32, i32)) -> bool {
        if self.state >= 2 {
            return false;
        }
        let (lx, ly) = self.px();
        let b = if self.beaten_box { BEATEN_BOX } else { BOX };
        let dy = (y - (b.2 + ly)) as u16;
        if dy.wrapping_add(0x0E) >= (b.3 + 0x0E) as u16 {
            return false;
        }
        let dx = (x - (b.0 + lx)) as u16;
        if dx.wrapping_add(8) >= (b.1 + 8) as u16 {
            return false;
        }
        if self.fire != 0 {
            self.fire -= 1;
            self.flash = 0x10;
        }
        true
    }

    /// 0x0808FB08 -> 0x0808DC68 for each ring (slots 5..0): the timer, gone off the screen,
    /// the move.
    fn step_rings(&mut self, camera: (i32, i32)) {
        for r in self.rings.iter_mut().rev() {
            let Some(ring) = r else { continue };
            ring.timer = ring.timer.saturating_sub(1);
            let (x, y) = ring.px();
            if ((x - camera.0 + 8) & 0xFFFF) >= 0x100 || ((y + 0x14 - camera.1) & 0xFFFF) >> 8 != 0 {
                *r = None;
                continue;
            }
            ring.x += ring.vx * 16;
            ring.y += ring.vy * 16;
        }
    }

    /// 0x0808CC9C for the rings: the player's frame top-left (px) and whether his box is
    /// big (not small, not crouching).
    pub fn ring_hits(&self, frame: (i32, i32), big: bool) -> bool {
        self.rings.iter().flatten().any(|r| r.hits(frame, big))
    }

    pub fn ring_touches(&self, player: &Player) -> bool {
        self.ring_hits(player.frame(), !player.small_box())
    }

    /// The star: rings touching the player leave their slots (as puffs, 0x0808CA84); their
    /// positions.
    pub fn take_rings_touching(&mut self, player: &Player) -> Vec<(i32, i32)> {
        let (frame, big) = (player.frame(), !player.small_box());
        let mut out = Vec::new();
        for r in &mut self.rings {
            if let Some(ring) = *r
                && ring.hits(frame, big)
            {
                out.push(ring.px());
                *r = None;
            }
        }
        out
    }

    /// The rings' OAM entries (0x0808DC68, slots 5..0; drawn after their move).
    pub fn ring_sprites(&self, camera: (i32, i32), counter: u8) -> Vec<ObjSprite> {
        let mut out = Vec::new();
        for (slot, r) in self.rings.iter().enumerate().rev() {
            let Some(r) = r else { continue };
            let (x, y) = r.px();
            let a0 = (((y - camera.1) & 0xFF) as u16) | 0x8000;
            let vflip = (((counter >> 2) & 1) ^ (slot as u8 & 1)) != 0;
            let a1 = ((x - camera.0) & 0x1FF) as u16 | if vflip { 0x2000 } else { 0 };
            let a2 = r.tile() | 0x0400 | (RING_PALETTE as u16) << 12;
            out.extend(ObjSprite::from_attrs(a0, a1, a2));
        }
        out
    }
}

impl Larry {
    /// 0x08072954: the spin, the wand in his hand (0x08072680), the body (0x080727B0 twice).
    fn draw(&mut self, camera: (i32, i32), held: bool) {
        if self.state == 0 && self.flash != 0 && self.flash & 3 < 2 {
            self.flash_palette = self.flash & 3 == 0;
        }
        if !held && self.shell != 0 && self.shell & 3 == 0 {
            let s = self.state as usize;
            self.frame += 1;
            if self.frame >= SPIN_END[s] {
                self.frame = SPIN_LOOP[s];
            }
            self.flags = SPIN_FLAGS[self.frame as usize];
        }
        self.sprites.clear();
        let right = self.facing_right();
        if self.frame < 4 {
            // 0x08072680.
            let f = self.frame as usize;
            let (wx, wy) = (self.x + WAND_DX[right as usize][f] * SUB, self.y + WAND_DY[f] * SUB);
            (self.off_h, self.off_v) = Self::off_screen(wx, wy, camera);
            let (sx, sy) = ((wx >> 8) - camera.0, (wy >> 8) - camera.1 + 1);
            let pal = match self.magic {
                0 => 1,
                m => match m & 6 {
                    4 => 0x0F,
                    k => k >> 1,
                },
            };
            if self.off_v & 1 == 0 && ((sx + 0x20) & 0xFFFF) < 0x120 {
                let [t0, t1] = WAND_TILES[f];
                let (t0, t1) = if right { (t1, t0) } else { (t0, t1) };
                for (k, (dx, tile)) in [(0, t0), (8, t1)].into_iter().enumerate() {
                    // Both halves take the facing (0x08046E94), then 0x08072680 flips one
                    // back: the right half always ends up mirrored.
                    let h = k == 1;
                    let a0 = ((sy & 0xFF) as u16) | 0x8000;
                    let a1 = ((sx + dx) & 0x1FF) as u16 | if h { 0x1000 } else { 0 };
                    let a2 = tile | 0x0400 | (pal as u16) << 12;
                    self.sprites.extend(ObjSprite::from_attrs(a0, a1, a2));
                }
            }
        }
        // 0x08046D58: +0xC / +0xE.
        self.screen = ((self.x >> 8) - camera.0, (self.y >> 8) - camera.1);
        if self.off_v & 1 != 0 || self.off_h & 0x0E != 0 {
            return;
        }
        let row = if self.frame < 4 { STAND_ROWS + if self.frame == 3 { 0 } else { self.frame as usize } } else { self.frame as usize } - 4;
        let tiles = BODY[row];
        let pal = if self.flash_palette { FLASH_PALETTE } else { PALETTE };
        let (sx, sy) = (self.screen.0, self.screen.1 + 1);
        let vflip = self.flags & 0x80 != 0;
        for (i, &tile) in tiles.iter().enumerate() {
            let (col, r) = ((i % 3) as i32, (i / 3) as i32);
            let mut y = sy + r * 0x10;
            let x = if right { sx + 0x10 - col * 8 } else { sx + col * 8 };
            let (mut h, mut v) = (right, vflip);
            if tile == 0xFFFF {
                continue;
            }
            match self.frame {
                // The middle column turns.
                0x0C | 0x10 if col == 1 => h = true,
                4..=9 if r == 0 && col != 1 => {
                    if self.shell < 0x60 {
                        continue;
                    }
                    y += 6;
                    if col == 0 {
                        (h, v) = (false, false);
                    } else {
                        (h, v) = (true, false);
                    }
                }
                _ => {}
            }
            let a0 = ((y & 0xFF) as u16) | 0x8000;
            let a1 = (x & 0x1FF) as u16 | if h { 0x1000 } else { 0 } | if v { 0x2000 } else { 0 };
            let a2 = (tile & 0x3FF) | 0x0400 | ((pal as u16) & 0xF) << 12;
            if let Some(mut s) = ObjSprite::from_attrs(a0, a1, a2) {
                s.pal = pal;
                self.sprites.push(s);
            }
        }
    }

    /// OAM entries this frame: the rings, then his wand and body (all behind the player).
    pub fn sprites(&self, camera: (i32, i32), counter: u8) -> Vec<ObjSprite> {
        let mut out = self.ring_sprites(camera, counter);
        out.extend(self.sprites.iter().copied());
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::TEST_SOLIDITY;
    use sma4_assets::BlockRules;

    /// 16x10: a floor on row 9.
    fn cabin() -> Level {
        let (w, h) = (16, 10);
        let mut m = vec![0x60; w * h];
        for x in 0..w {
            m[(h - 1) * w + x] = 0x53;
        }
        Level::new(w, h, m, TEST_SOLIDITY, BlockRules::default())
    }

    fn view(x: i32, y: i32) -> PlayerView {
        PlayerView { x: x * SUB, y: y * SUB, vy: 0, on_ground: true, small: true, ignore: false }
    }

    const CAMERA: (i32, i32) = (8, 0);

    #[test]
    fn the_aim_counts_with_an_8_bit_accumulator_as_the_rom_does() {
        // Recorded in mGBA: Larry at (0xD0, 0x80), the player at (0x68, 0x27) (his row is
        // taken as 0x50): the rings fly at x -20, y -1 (not -9).
        let mut l = Larry::spawn(0xD0, 0x80);
        l.aim_at(&view(0x68, 0x27));
        assert_eq!(l.aim, (-0x14, -1));
    }

    #[test]
    fn he_walks_jumps_and_casts_three_rings() {
        let level = cabin();
        let mut l = Larry::spawn(0xA0, 0x70);
        let p = view(0x20, 0x70);
        let (mut jumped, mut rings) = (false, 0);
        for counter in 1..=0x80u32 {
            l.step(&level, &p, CAMERA, counter as u8, false);
            jumped |= l.vy < 0;
            rings = rings.max(l.rings.iter().flatten().count());
        }
        assert!(jumped, "jumps when the counter & 0xBF is 0");
        assert_eq!(rings, 3, "three rings a cast");
        assert!(l.rings.iter().flatten().all(|r| r.vx < 0), "toward the player");
    }

    #[test]
    fn three_stomps_beat_him_and_he_flies_off_for_the_wand() {
        let level = cabin();
        let mut l = Larry::spawn(0xA0, 0x70);
        let mut stomps = 0;
        let mut beaten = false;
        for counter in 1..2000u32 {
            let (lx, ly) = l.px();
            // Coming down on him whenever he is out of his shell.
            let mut p = view(lx + 4, ly - 0x14);
            p.on_ground = false;
            p.vy = 0x20;
            let p = if l.state == 0 && l.shell == 0 { p } else { view(0x10, 0x70) };
            let ev = l.step(&level, &p, CAMERA, counter as u8, false);
            assert!(!ev.hurt_player);
            stomps += ev.stomped as u32;
            beaten |= ev.beaten;
            // The spin steps one frame every 4 (0x0816F633 / 0x0816F635): from his standing
            // frames it reaches the shell's (4..9) within 0x10.
            if l.state == 0 && (2..=0x70).contains(&l.shell) {
                assert!((4..10).contains(&l.frame), "spinning in his shell");
            }
            if ev.wand {
                assert_eq!((stomps, l.state), (3, 2));
                assert!(beaten, "the clock stops at the third");
                assert!(l.px().1 < -0x40 && (l.px().0 & 0xFF) < 0x88, "off the top near the middle");
                return;
            }
        }
        panic!("no wand: {stomps} stomps, state {}", l.state);
    }

    #[test]
    fn walking_into_him_hurts_and_ten_fireballs_beat_him() {
        let level = cabin();
        let mut l = Larry::spawn(0xA0, 0x70);
        l.step(&level, &view(0x20, 0x70), CAMERA, 1, false);
        let ev = l.step(&level, &view(0x9C, 0x70), CAMERA, 2, false);
        assert!(ev.hurt_player && !ev.stomped);
        let (lx, ly) = l.px();
        for i in 0..FIRE_HITS {
            assert!(l.fireball((lx + 8, ly + 12)));
            assert_eq!(l.flash, 0x10);
            assert_eq!(l.fire, FIRE_HITS - 1 - i);
        }
        let ev = l.step(&level, &view(0x20, 0x70), CAMERA, 3, false);
        assert!(ev.beaten && l.state == 1);
    }
}
