//! The magic wand at the end of an airship (Larry Koopa's cabin, level 0x213) and the
//! "LEVEL CLEARED!" sequence once Mario catches it. See docs/re-notes.md "The wand and
//! LEVEL CLEARED".
//!
//! * The wand is projectile type 0x10 (0x0807230C: the first free of slots 5..0, x 0x80,
//!   y 0x0A, speeds 0, +0x13 = 0x50), handled by 0x0808ED40: it moves (0x0808D0E4: y +=
//!   speed * 16, speed + 2 below 0x6E), then speed - 1, so it falls 1/16 px a frame faster
//!   each frame; it spins by adding +0x13 to 0x03007848 + 0xBEF, a carry turning the frame
//!   0x03007848 + 0xBF0. Landing on a solid cell under (x + 4, y + 0x0C) (0x0808D108) at
//!   0x20 or more it bounces at half the speed, 2 px up, +0x13 - 0x80 (sound 0x58 the
//!   first time, 0xBF1 + 1); slower it stops (speed 0, 0xBF0 / 0xBF1 = 0, y's pixel low
//!   nibble 5).
//! * 0x0808EBB0 draws it (two tall 8x16 OBJs, palette 1, priority 1, tiles 0x08170B38 /
//!   0x08170B40[frame & 7] (frame ^ 7 after an odd number of bounces), the right one
//!   h-flipped, both v-flipped by 0x08170B48 bit 7) and tests the player (0x0808CC9C).
//! * Caught (0x0808CAEC): Larry's state 0x030038C6 goes 2 -> 3, the clock stops
//!   (0x03003BB0, 0x03003771), music 0x21, the tally delay 0x03003BB4 = 0x160, the clock
//!   digits are kept at 0x03007848 + 0xDC4 for the tally line, the wand's slot is freed.
//! * State 3 (0x08073210): player speed 0, +0xBEB = 0x9E / +0xBEC = 2 (a backdrop flash,
//!   0x08089150, that the cabin's walls hide), state 4.
//! * State 4 (0x0807336C): with Larry's +0x23 at 0, text 0xAF ("LEVEL CLEARED!") when the
//!   delay is 0x50, and the tally 0x08066F08(obj, 4, 1): the delay counts down to 0x10,
//!   the line "<clock icon><kept clock>x200= <clock x 200>" is written while it is below
//!   0x30 (0x08066C3C), then the clock drains (2000 points per ten seconds, 200 per second,
//!   0x0816EE44[4..6]); empty: sound 0x1F, the delay 0. Then +0x23 = 0x30; at 1 it becomes
//!   0xC0 and state 5 begins.
//! * State 5 (0x080733F8): the texts are erased, then the BG camera flickers onto the
//!   cabin's empty rows (the room disappears into black). State 6 (0x08073510): clouds
//!   (objects 0x08071E5C / 0x08072040) rise from below and the backdrop fades in to the
//!   sky's 0x7EED (0x08071DBC). State 7 (0x08073548): the player falls 2 px a frame; the
//!   level ends once his frame y reaches 0x200.
//! * While the state is 3 or more, the player is held (0x08094BD8) in pose 0x0813DEE8[form]
//!   (the jump pose) and 0x080A83EC draws the wand held over his head.

use crate::level::{Level, TILE_PX};
use crate::obj::ObjSprite;
use crate::player::{Player, SUB};
use sma4_assets::TileKind;

/// Larry's cabin.
pub const CABIN: u16 = 0x213;

/// 0x0807230C: where the wand appears (level pixels) and its spin step +0x13.
const START: (i32, i32) = (0x80, 0x0A);
const SPIN: u8 = 0x50;
/// 0x0808D0E4: gravity +2 while the speed is below 0x6E; 0x0808ED40 then takes 1 off.
const GRAVITY_CAP: i32 = 0x6E;
/// 0x0808D108's probe: (x + 4, y + 0x0C).
const PROBE: (i32, i32) = (4, 0x0C);
/// 0x0808ED40: landing at this speed or more bounces.
const BOUNCE_SPEED: i32 = 0x20;
const BOUNCE_UP: i32 = 2 * SUB;
const BOUNCE_SPIN: u8 = 0x80;
/// Resting: y's pixel low nibble.
const REST_Y: i32 = 5;
/// 0x0808EBB0: tiles of the left and right halves by spin frame, v-flip in bit 7.
const LEFT_TILES: [u16; 8] = [0x97, 0x98, 0x9A, 0x98, 0x97, 0x99, 0x9B, 0x99];
const RIGHT_TILES: [u16; 8] = [0x97, 0x99, 0x9B, 0x99, 0x97, 0x98, 0x9A, 0x98];
const VFLIP: [bool; 8] = [false, false, false, true, true, true, false, false];
const PALETTE: u8 = 1;
const PRIORITY: u8 = 1;
/// OBJ y is the wand's y - camera - 4.
const DRAW_DY: i32 = -4;
/// 0x0808CC9C: (y - frame y - TOP[big] + 8) as u16 < H[big] (0x08170A9C / 0x08170A9E),
/// (x - frame x + 6) as u16 < 0x10; big = not small and not crouching.
const TOUCH_TOP: [i32; 2] = [0x12, 0x0A];
const TOUCH_H: [i32; 2] = [0x10, 0x16];
const TOUCH_DX: i32 = 6;
const TOUCH_W: i32 = 0x10;

/// The falling wand (projectile 0x10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Wand {
    /// Position, 1/256 px (+0 / +4).
    pub x: i32,
    pub y: i32,
    /// Speed byte (+8), 1/16 px a frame.
    pub vy: i32,
    /// Spin step +0x13, accumulator 0xBEF, frame 0xBF0, bounces 0xBF1.
    spin: u8,
    spin_acc: u8,
    pub spin_frame: u8,
    bounces: u8,
}

impl Wand {
    /// Dropped by Larry (0x0807230C). The projectile runs in the frame it appears: call
    /// `step` once that frame.
    pub fn spawn() -> Self {
        Self { x: START.0 * SUB, y: START.1 * SUB, vy: 0, spin: SPIN, spin_acc: 0, spin_frame: 0, bounces: 0 }
    }

    /// One frame of 0x0808ED40 (without the fireball and tail hits, 0x0808E9B0 /
    /// 0x0808EAA4, that bounce it up, nor its sparkles, 0x0809137C).
    pub fn step(&mut self, level: &Level) {
        self.y += self.vy * 16;
        if self.vy < GRAVITY_CAP {
            self.vy += 2;
        }
        self.vy -= 1;
        let (acc, carry) = self.spin_acc.overflowing_add(self.spin);
        self.spin_acc = acc;
        if carry {
            self.spin_frame = self.spin_frame.wrapping_add(1);
        }
        let (px, py) = (self.x.div_euclid(SUB) + PROBE.0, self.y.div_euclid(SUB) + PROBE.1);
        if level.at(px.div_euclid(TILE_PX), py.div_euclid(TILE_PX)) == TileKind::Solid && self.vy >= 0 {
            if self.vy < BOUNCE_SPEED {
                self.vy = 0;
                self.spin_frame = 0;
                self.bounces = 0;
                self.y = (self.y & !(0xF * SUB)) + REST_Y * SUB;
            } else {
                self.vy = -(self.vy >> 1);
                self.y -= BOUNCE_UP;
                self.bounces = self.bounces.wrapping_add(1);
                self.spin = self.spin.wrapping_sub(BOUNCE_SPIN);
            }
        }
    }

    /// 0x0808FB08 frees the slot once it is off the screen sideways or below it.
    pub fn gone(&self, camera: (i32, i32)) -> bool {
        let dx = (self.x.div_euclid(SUB) - camera.0 + 8) as u16;
        let dy = ((self.y.div_euclid(SUB) + 0x14 - camera.1) as i16) >> 8;
        dx >= 0x100 || dy > 0
    }

    /// 0x0808EBB0's OAM entries.
    pub fn sprites(&self, camera: (i32, i32)) -> Vec<ObjSprite> {
        let mut k = (self.spin_frame & 7) as usize;
        if self.bounces & 1 != 0 {
            k ^= 7;
        }
        let x = self.x.div_euclid(SUB) - camera.0;
        let y = (self.y.div_euclid(SUB) - camera.1 + DRAW_DY) as u8 as i32;
        let piece = |x: i32, tile: u16, hflip: bool| ObjSprite {
            x: wrap_x(x),
            y: if y >= 0xC0 { y - 0x100 } else { y },
            tile,
            shape: 2,
            size: 0,
            pal: PALETTE,
            hflip,
            vflip: VFLIP[k],
            prio: PRIORITY,
            blend: false,
        };
        vec![piece(x, LEFT_TILES[k], false), piece(x + 8, RIGHT_TILES[k], true)]
    }

    /// 0x0808CC9C: the player touches it.
    pub fn touches(&self, player: &Player) -> bool {
        let (fx, fy) = player.frame();
        let big = (!player.small_box()) as usize;
        let (x, y) = (self.x.div_euclid(SUB), self.y.div_euclid(SUB));
        ((y - fy - TOUCH_TOP[big] + 8) as u16) < TOUCH_H[big] as u16 && ((x - fx + TOUCH_DX) as u16) < TOUCH_W as u16
    }
}

fn wrap_x(x: i32) -> i32 {
    let x = x & 0x1FF;
    if x >= 0x100 { x - 0x200 } else { x }
}

/// The tally delay 0x03003BB4 the catch sets, the delay at which "LEVEL CLEARED!" is
/// written, below which the tally line shows, and at which the clock starts draining.
const TALLY_DELAY: u16 = 0x160;
const CLEAR_TEXT_AT: u16 = 0x50;
const LINE_BELOW: u16 = 0x30;
const DRAIN_AT: u16 = 0x10;
/// 0x0816EE44[4] / [5]: tens of points per ten seconds and per second drained.
pub const POINTS: [u32; 2] = [200, 20];
/// Larry's +0x23 after the tally (0x0807336C), and the value that ends state 4.
const AFTER_TALLY: u8 = 0x30;

/// State 5: Larry's +0x23 from 0xC0; at 0x80 the texts are erased (0x08073254 writes
/// blank cells over BG0's rows), below it the BG camera y (0x03003754 / 0x0300375C) is
/// 0xEF (the cabin's empty rows: black) while +0x23 & 0x0816F898[+0x23 >> 5] is set,
/// else 0x18: the room flickers away, faster and faster.
const SHAKE: u8 = 0xC0;
const ERASE_AT: u8 = 0x80;
const SHAKE_MASKS: [u8; 4] = [1, 2, 4, 8];
/// State 6: +0x23 from 0xFF; the backdrop fades in below 0x9A.
const SKY: u8 = 0xFF;
const FADE_BELOW: u8 = 0x9A;
/// State 7: 0x080A8000 drops the held player 2 px a frame; the level ends once his frame
/// y reaches 0x200 (0x08073548: player +6 > 1).
pub const FALL_STEP: i32 = 2;
const FALL_END: i32 = 0x200;
/// 0x08071DBC fades colour 0 of the BG palette (the backdrop) up to 0x7EED through the
/// colour curve (0x08007B5C, row 0 as the port's palettes: 0x08132DE4 = identity at these
/// values): green and blue + 1 on frames with (0x03003AE8 & 3) == 0, red on & 7 == 0.
pub const SKY_COLOUR: u16 = 0x7EED;

/// The clouds (0x08071E5C: object slots 4..0 of 0x03007848, positions in screen pixels):
/// x 0x0816F60C[k], y 0x100 + 0x0816F614[k] with k = (frame & 3) + i, speed -1.
const CLOUD_X: [u8; 8] = [0x30, 0x18, 0xC0, 0x58, 0x80, 0x98, 0x18, 0x70];
const CLOUD_Y: [u8; 8] = [0x10, 0x48, 0x68, 0x70, 0x98, 0x00, 0x30, 0x58];
/// 0x08072040: rising speed by slot (halved while +0x23 is not 0), and columns - 1 by
/// slot (0x0816F621).
const CLOUD_SPEED: [i8; 5] = [-0x40, -0x2B, -0x37, -0x2D, -0x21];
const CLOUD_COLUMNS: [u8; 5] = [3, 2, 2, 3, 3];
/// Above -0x10 px a cloud comes back at y 0xC8 and x 0x0816F60C[((frame >> 5) & 3) + i].
const CLOUD_TOP: i32 = -0x1000;
const CLOUD_BACK_Y: i32 = 0xC800;
/// 0x08071EC4: 8x8 OBJs, the top row tile 0x10B (palette 0), the bottom row 0x12B
/// (palette 6), the end columns 0x10A / 0x12A (the right one h-flipped), priority 1.
const CLOUD_TOP_TILES: [u16; 2] = [0x10A, 0x10B];
const CLOUD_BOTTOM_TILES: [u16; 2] = [0x12A, 0x12B];
const CLOUD_PALETTES: [u16; 2] = [0, 6];

/// One cloud: x and y in 1/256 screen px (+0 / +8), speed (+0x12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Cloud {
    pub x: i32,
    pub y: i32,
    pub vy: i8,
}

impl Cloud {
    /// 0x08071EC4's OAM entries for slot `slot`.
    fn sprites(&self, slot: usize, out: &mut Vec<ObjSprite>) {
        if self.y >= 0x1F000 || ((self.y >> 8) as i16).wrapping_add(0x20) as u16 >= 0xE0 {
            return;
        }
        let y = (self.y >> 8) as u8 as u16;
        let x = ((self.x & 0xFF_FFFF) >> 8) as u16;
        let n = CLOUD_COLUMNS[slot] as u16 + 1;
        for c in 0..n {
            let end = c == 0 || c == n - 1;
            let flip = if c == n - 1 { 0x1000 } else { 0 };
            let cx = (x + 8 * c) & 0xFF;
            for (row, tiles) in [CLOUD_TOP_TILES, CLOUD_BOTTOM_TILES].iter().enumerate() {
                let tile = tiles[!end as usize];
                let a2 = tile | 1 << 10 | CLOUD_PALETTES[row] << 12;
                out.extend(ObjSprite::from_attrs((y + 8 * row as u16) & 0xFF, cx | flip, a2));
            }
        }
    }
}

/// Larry's states 3-7 (0x030038C6) after the catch: the tally, then the cabin goes and
/// Mario falls through the sky.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cleared {
    /// 0x030038C6: 3 the frame after the catch, then 4..7.
    pub state: u8,
    /// 0x03003BB4.
    delay: u16,
    /// Larry's +0x23 (counted down before his main by the object loop).
    timer: u8,
    /// The clock when the wand was caught (0x03007848 + 0xDC4..).
    pub kept: [u8; 3],
    text: bool,
    /// The tally is over.
    tallied: bool,
    /// The texts were erased (state 5).
    erased: bool,
    /// The BG camera is at 0xEF: the level's layers show nothing.
    pub level_hidden: bool,
    /// BG colour 0 (BGR555) from state 5 on.
    pub backdrop: u16,
    /// Object slots 0..4 from state 6.
    pub clouds: Option<[Cloud; 5]>,
}

/// What the sequence asks of the game this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Events {
    /// Tens of points the drain adds.
    pub points: u32,
    /// The level ends (0x08073548: 0x03003851 = 0, 0x03003AE7 + 1).
    pub done: bool,
}

impl Cleared {
    /// The catch (0x0808CAEC), with the clock as it stopped.
    pub fn new(clock: [u8; 3]) -> Self {
        Self {
            state: 3,
            delay: TALLY_DELAY,
            timer: 0,
            kept: clock,
            text: false,
            tallied: false,
            erased: false,
            level_hidden: false,
            backdrop: 0,
            clouds: None,
        }
    }

    /// One frame of Larry's main for states 3-7: `clock` drains into the score, `frame`
    /// is the frame counter (0x03003AE8), `player_y` the held player's frame y (px).
    pub fn step(&mut self, clock: &mut [u8; 3], frame: u32, player_y: i32) -> Events {
        let mut ev = Events::default();
        self.timer = self.timer.saturating_sub(1);
        match self.state {
            // 0x08073210.
            3 => self.state = 4,
            // 0x0807336C.
            4 => {
                if self.timer == 0 {
                    if self.delay == CLEAR_TEXT_AT {
                        self.text = true;
                    }
                    if !self.tally(clock, &mut ev) {
                        self.timer = AFTER_TALLY;
                    }
                } else if self.timer == 1 {
                    self.timer = SHAKE;
                    self.state = 5;
                }
            }
            // 0x080733F8 (Larry's branch: his +0x18 is not 0x114).
            5 => {
                let t = self.timer;
                if t == 0 {
                    // Music 0x26, the clouds (0x08071E5C), the player in the air.
                    self.clouds = Some(std::array::from_fn(|i| {
                        let k = (frame as usize & 3) + i;
                        Cloud { x: (CLOUD_X[k] as i32) << 8, y: (CLOUD_Y[k] as i32) << 8 | 0x1_0000, vy: -1 }
                    }));
                    self.timer = SKY;
                    self.state = 6;
                } else if t >= ERASE_AT {
                    if t == ERASE_AT {
                        self.erased = true;
                    }
                } else {
                    self.level_hidden = t & SHAKE_MASKS[(t >> 5) as usize] != 0;
                }
            }
            // 0x08073510.
            6 => {
                if self.timer < FADE_BELOW {
                    self.fade(frame);
                }
                if self.timer == 0 {
                    self.state = 7;
                }
                self.step_clouds(frame);
            }
            // 0x08073548.
            _ => {
                self.fade(frame);
                self.step_clouds(frame);
                ev.done = player_y >= FALL_END;
            }
        }
        ev
    }

    /// 0x08071DBC.
    fn fade(&mut self, frame: u32) {
        let ch = |c: u16, s: u16| (c >> s) & 0x1F;
        let (mut r, mut g, mut b) = (ch(self.backdrop, 0), ch(self.backdrop, 5), ch(self.backdrop, 10));
        if frame & 3 == 0 {
            g += 1;
            b += 1;
        }
        if frame & 7 == 0 {
            r += 1;
        }
        r = r.min(ch(SKY_COLOUR, 0));
        g = g.min(ch(SKY_COLOUR, 5));
        b = b.min(ch(SKY_COLOUR, 10));
        self.backdrop = b << 10 | g << 5 | r;
    }

    /// 0x08072040: each cloud (slots 4..0) is drawn, moves, takes its speed and comes
    /// back below once above the screen.
    fn step_clouds(&mut self, frame: u32) {
        let half = self.timer != 0;
        let Some(clouds) = &mut self.clouds else { return };
        let base = ((frame >> 5) & 3) as usize;
        for (i, c) in clouds.iter_mut().enumerate() {
            let slot = 4 - i;
            // 0x08049EA0: falling speeds above 0x40 are capped (never here).
            if c.vy > 0x40 {
                c.vy = 0x40;
            }
            c.y += c.vy as i32 * 16;
            let target = if half { CLOUD_SPEED[slot] >> 1 } else { CLOUD_SPEED[slot] };
            c.vy = c.vy.min(target);
            if c.y < CLOUD_TOP {
                c.x = (CLOUD_X[base + i] as i32) << 8;
                c.y = CLOUD_BACK_Y;
            }
        }
    }

    /// The clouds' OAM entries as drawn this frame (0x08071EC4 runs before each moves:
    /// call before `step`).
    pub fn cloud_sprites(&self) -> Vec<ObjSprite> {
        let mut out = Vec::new();
        if let Some(clouds) = &self.clouds {
            for (i, c) in clouds.iter().enumerate() {
                c.sprites(4 - i, &mut out);
            }
        }
        out
    }

    /// 0x08066F08(obj, 4, 1): true while it is busy.
    fn tally(&mut self, clock: &mut [u8; 3], ev: &mut Events) -> bool {
        if self.tallied {
            return false;
        }
        if self.delay != DRAIN_AT {
            self.delay -= 1;
            return true;
        }
        if clock.iter().all(|&d| d == 0) {
            // 0x0806894C, sound 0x1F, the delay to 0.
            self.tallied = true;
            return false;
        }
        ev.points += crate::goal::drain(clock, POINTS);
        true
    }

    /// "LEVEL CLEARED!" is on the screen.
    pub fn text(&self) -> bool {
        self.text && !self.erased
    }

    /// The tally line, once written (0x08066C3C runs while the delay is below 0x30 and
    /// during the drain): the kept clock, then the drained clock's points.
    pub fn line(&self, clock: &[u8; 3]) -> Option<TallyLine> {
        (!self.erased && (self.tallied || self.delay < LINE_BELOW))
            .then(|| TallyLine { kept: self.kept, points: tally_points(clock) })
    }

    /// The player falls this frame (0x080A8000 with 0x030038C6 >= 7, before the objects).
    pub fn falling(&self) -> bool {
        self.state >= 7
    }
}

/// The value the line shows after "=": the clock left x 0x0816EE44[5] (tens; the line's
/// last cell is a fixed 0).
fn tally_points(clock: &[u8; 3]) -> u32 {
    POINTS[1] * (clock[0] as u32 * 100 + clock[1] as u32 * 10 + clock[2] as u32)
}

/// One BG0 cell of the tally line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Glyph {
    /// Yellow digit (palette 0, tiles 0x290-0x299).
    Digit(u8),
    /// White digit (palette 15).
    White(u8),
    /// 0x29A.
    Times,
    /// The clock icon 0x29B.
    Clock,
    /// 0x282.
    Equals,
    Blank,
}

/// The tally line 0x08066C3C writes at BG0 cell (8, 8): 15 cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TallyLine {
    pub kept: [u8; 3],
    /// Tens of points.
    pub points: u32,
}

/// Where the line starts (BG0 cell; 0x0816EE78[4] = 0x200 + 0x10 bytes into the map).
pub const LINE_AT: (u32, u32) = (8, 8);
/// 0x0816EE86[4..6] and the cell after: "200".
const MULTIPLIER: [u8; 3] = [2, 0, 0];

impl TallyLine {
    pub fn cells(&self) -> Vec<Glyph> {
        let mut out = vec![Glyph::Clock];
        out.extend(self.kept.iter().map(|&d| Glyph::Digit(d)));
        out.push(Glyph::Times);
        out.extend(MULTIPLIER.iter().map(|&d| Glyph::White(d)));
        out.push(Glyph::Equals);
        // Five digits without leading zeros (10000 down to 1), then the fixed 0.
        let mut v = self.points % 100_000;
        let mut lead = true;
        for place in [10_000, 1_000, 100, 10, 1] {
            let d = (v / place) as u8;
            v %= place;
            if lead && d == 0 {
                out.push(Glyph::Blank);
            } else {
                lead = false;
                out.push(Glyph::White(d));
            }
        }
        out.push(Glyph::White(0));
        out
    }
}

/// 0x080A83EC: the wand over the held player's head, two tall 8x16 OBJs of tile 0x97
/// (the right one h-flipped), priority 1, palette 0x08169594[(frame >> 1) & 3] (2 means
/// 15). `screen` is the player frame's top-left on the screen.
pub fn held_sprites(player: &Player, screen: (i32, i32), frame: u32) -> Vec<ObjSprite> {
    // 0x081737C8[(+0x60 != 0) * 2] (Mario): -6 facing left, +6 facing right.
    let x = screen.0 + if player.facing_left { -6 } else { 6 };
    // Above the frame: 1 for the small form, -9 for Mario's others (-0x0D for Luigi).
    let y = screen.1 + if player.form == crate::player::Form::Small { 1 } else { -9 };
    if ((y + 0x10) as u16) >= 0xB0 {
        return Vec::new();
    }
    let pal = match [0u8, 1, 2, 1][((frame >> 1) & 3) as usize] {
        2 => 15,
        p => p,
    };
    let piece = |x: i32, hflip: bool| ObjSprite {
        x: wrap_x(x),
        y: (y + 1) as u8 as i32,
        tile: 0x97,
        shape: 2,
        size: 0,
        pal,
        hflip,
        vflip: false,
        prio: 1,
        blend: false,
    };
    vec![piece(x, false), piece(x + 8, true)]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A room with a solid floor at row 10 (y 0xA0), as the cabin's.
    fn floor() -> Level {
        let (w, h) = (32, 16);
        let mut kinds = vec![TileKind::Empty; w * h];
        for k in kinds[10 * w..11 * w].iter_mut() {
            *k = TileKind::Solid;
        }
        Level::from_kinds(w, h, &kinds)
    }

    #[test]
    fn the_wand_falls_bounces_twice_and_rests() {
        let level = floor();
        let mut w = Wand::spawn();
        w.step(&level);
        // Recorded in mGBA: after the spawn frame, speed 1, accumulator 0x50.
        assert_eq!((w.y, w.vy, w.spin_acc), (0xA00, 1, 0x50));
        let mut bounces = Vec::new();
        for f in 1..200 {
            let before = w.vy;
            w.step(&level);
            if w.vy < 0 && before > 0 {
                bounces.push((f, w.vy));
            }
        }
        // Two bounces, then it rests with y's pixel nibble 5 (mGBA: 0x95A0).
        assert_eq!(bounces.len(), 2);
        assert_eq!((w.y, w.vy, w.spin_frame), (0x95A0, 0, 0));
        // Off its first bounce at 0x20 or more: half the speed (mGBA: -33 at frame 66).
        assert!(bounces[0].1 <= -0x10);
    }

    #[test]
    fn the_tally_waits_writes_the_text_then_drains_200_points_a_second() {
        let mut clock = [2, 8, 7];
        let mut c = Cleared::new(clock);
        let mut points = 0;
        let mut text_at = None;
        let mut line_at = None;
        let mut f = 0;
        // mGBA (frames after the catch): state 4 at 1, the text at 274, the line from 306,
        // the clock empty at 372, state 5 at 420.
        let end = loop {
            f += 1;
            let ev = c.step(&mut clock, f, 0x80);
            points += ev.points;
            if c.text() && text_at.is_none() {
                text_at = Some(f);
            }
            if c.line(&clock).is_some() && line_at.is_none() {
                line_at = Some(f);
            }
            assert!(!ev.done);
            if c.state == 5 {
                break f;
            }
            assert!(f < 1000);
        };
        assert_eq!((text_at, line_at, end), (Some(274), Some(306), 420));
        assert_eq!(points * 10, 287 * 200);
        assert_eq!(clock, [0, 0, 0]);
    }

    #[test]
    fn the_room_flickers_away_the_sky_fades_in_and_mario_falls_out() {
        let mut clock = [0, 0, 1];
        let mut c = Cleared::new(clock);
        let mut f = 0u32;
        while c.state != 5 {
            f += 1;
            c.step(&mut clock, f, 0x80);
        }
        let five = f;
        // mGBA (frames after the catch, state 5 at 420): the texts go at 484, the room
        // is hidden from 485 for 8 frames, back for 8, ...; state 6 at 612.
        let mut hidden = Vec::new();
        while c.state == 5 {
            f += 1;
            c.step(&mut clock, f, 0x80);
            assert_eq!(c.text() || c.line(&clock).is_some(), f - five < 0x40, "frame {f}");
            hidden.push(c.level_hidden);
        }
        assert_eq!(f - five, 0xC0);
        assert!(!hidden[..0x40].iter().any(|&h| h));
        assert!(hidden[0x40..0x48].iter().all(|&h| h) && !hidden[0x48..0x50].iter().any(|&h| h));
        assert!(c.level_hidden && c.clouds.is_some() && c.backdrop == 0);
        // State 6: 0xFF frames, the backdrop fading in once +0x23 is below 0x9A.
        let six = f;
        let mut y = 0x80;
        let mut fell = 0;
        loop {
            f += 1;
            if c.falling() {
                y += FALL_STEP;
                fell += 1;
            }
            if c.step(&mut clock, f, y).done {
                break;
            }
            if f - six < 0x65 {
                assert_eq!(c.backdrop, 0);
            }
            assert!(f - six < 1000);
        }
        // State 7 at 867, the level ends at 1059 (y 0x80 to 0x200).
        assert_eq!(f - six, 0xFF + 0xC0);
        assert_eq!(fell, 0xC0);
        assert_eq!(c.backdrop, SKY_COLOUR);
    }

    #[test]
    fn clouds_rise_at_their_slot_speed_and_come_back_below() {
        let mut c = Cleared::new([0; 3]);
        c.state = 7;
        c.clouds = Some([Cloud { x: 0x3000, y: -0xF00, vy: -0x40 }; 5]);
        c.step(&mut [0; 3], 0, 0);
        let clouds = c.clouds.unwrap();
        // Slot 4 (first) at -0x21, slot 0 (last) at -0x40: both above -0x10 px.
        assert_eq!((clouds[0].x, clouds[0].y), ((CLOUD_X[0] as i32) << 8, CLOUD_BACK_Y));
        assert_eq!(clouds[0].vy, -0x40, "min(-0x40, -0x21)");
        // Four columns of two 8x8 OBJs for slot 4, the right end mirrored.
        let mut out = Vec::new();
        Cloud { x: 0x3000, y: 0x1000, vy: 0 }.sprites(4, &mut out);
        assert_eq!(out.len(), 8);
        assert_eq!((out[0].tile, out[1].tile, out[2].tile, out[7].tile), (0x10A, 0x12A, 0x10B, 0x12A));
        assert!(out[7].hflip && !out[0].hflip);
        assert_eq!((out[1].pal, out[1].y, out[6].x), (6, 0x18, 0x48));
    }

    #[test]
    fn the_line_shows_the_kept_clock_and_the_points_left() {
        let line = TallyLine { kept: [2, 8, 7], points: 20 * 267 };
        use Glyph::*;
        assert_eq!(
            line.cells(),
            vec![
                Clock, Digit(2), Digit(8), Digit(7), Times, White(2), White(0), White(0), Equals, Blank, White(5),
                White(3), White(4), White(0), White(0)
            ]
        );
        let zero = TallyLine { kept: [2, 8, 7], points: 0 };
        assert_eq!(&zero.cells()[9..], &[Blank, Blank, Blank, Blank, Blank, White(0)]);
    }
}
