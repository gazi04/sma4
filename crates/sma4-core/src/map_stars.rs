//! The stars of a world start: after the "WORLD n" panel closes, eight stars spin in a
//! circle that grows, then shrinks while its centre flies to Mario; Mario appears where it
//! closes. Map intro (map sub-state 0, 0x0800EB0C) step 2 -> 0x08030738, table 0x08142688.
//!
//! * Init 0x0802FF40 (sound 0x22): angles 0x0813A65E (0x14, 0x10, ... 0x18, out of 32 per
//!   turn); the centre +0x3C / +0x3D starts at screen (0x70, 0x44) and heads for Mario's
//!   map sprite (x + scroll - 0x10, y - 0x10 - scroll); 0x0802FE84 splits each distance
//!   into 23 steps (quotient 0x03003C8B / C8C, remainder counters +0x3A / +0x45, one extra
//!   pixel while they are not below 0). Tile 0x8C, radius +0x36 = 0.
//! * Each frame 0x080304D0: the radius grows by 4 up to 0x5C (23 frames); then the centre
//!   moves one step a frame while the radius shrinks by 4; at 0 the stars are gone.
//!   +0x41 += 0x20: every 8 frames the tile swaps (0x0813A676: 0x8C, 0x8E); +0x3B += 0x70:
//!   on its carry every angle advances by one.
//! * Position 0x080302A4: offset = high byte of 2 * radius * 0x0813A666[angle & 15] (a half
//!   sine, 0..0x80; 0x080301A4), added for angles 0-15 and taken away for 16-31 on x; y
//!   uses angle + 8 (a quarter turn later). A star that wraps round the byte-sized screen
//!   (0x08030240) is hidden. OAM 0x08030488: 16x16 at (x, y), palette 2.

/// Starting angles (0x0813A65E).
const ANGLES: [u8; 8] = [0x14, 0x10, 0x0C, 0x08, 0x04, 0x00, 0x1C, 0x18];
/// Half a sine wave in 16 steps, 0x80 at the top (0x0813A666).
const SINE: [u8; 16] = [0x00, 0x18, 0x30, 0x47, 0x5A, 0x6A, 0x76, 0x7D, 0x80, 0x7D, 0x76, 0x6A, 0x5A, 0x47, 0x30, 0x18];
/// Where the circle starts (0x0802FF40, +0x4F = 2).
const START: (u8, u8) = (0x70, 0x44);
/// The flight to Mario takes this many steps (0x0802FE84 divides by 0x17).
const STEPS: u8 = 0x17;
const RADIUS_STEP: u8 = 4;
/// The radius grows while below this (0x080304D0).
const RADIUS_TOP: u8 = 0x5F;
const TILE_TICK: u8 = 0x20;
const SPIN: u8 = 0x70;
/// Star tiles (0x0813A676): cells of `sma4_assets` `world{n}_stars.png`.
pub const STAR_FRAMES: usize = 2;
/// The circle closes this far left of Mario's map sprite (0x0802FF40).
const TARGET_OFFSET: u8 = 0x10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Axis {
    pos: u8,
    target: u8,
    step: u8,
    extra: i8,
}

impl Axis {
    /// 0x0802FE84: the distance split into 23 steps.
    fn new(pos: u8, target: u8) -> Self {
        let d = pos.abs_diff(target);
        Self { pos, target, step: d / STEPS, extra: (d % STEPS) as i8 }
    }

    /// One step of 0x080304D0.
    fn advance(&mut self) {
        self.extra = self.extra.wrapping_sub(1);
        let extra = (self.extra >= 0) as u8;
        self.pos = if self.pos < self.target {
            self.pos.wrapping_add(self.step).wrapping_add(extra)
        } else {
            self.pos.wrapping_sub(self.step).wrapping_sub(extra)
        };
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapStars {
    x: Axis,
    y: Axis,
    radius: u8,
    shrinking: bool,
    angles: [u8; 8],
    /// +0x41 (tile swap) and +0x3B (spin) accumulators, and the tile toggle +0x42.
    tile_timer: u8,
    spin: u8,
    tile: usize,
    /// Screen positions (top-left of each 16x16 star) written this frame.
    shown: [Option<(u8, u8)>; 8],
    pub done: bool,
}

impl MapStars {
    /// 0x0802FF40. `mario` is the screen position of Mario's map sprite (its lower 16x16:
    /// map y - 0x10 - scroll); the circle closes 16 px left of it. (The ROM adds the map's
    /// x scroll 0x03003750 where the sprite takes it away; 0 on one-screen maps.)
    pub fn new(mario: (i32, i32)) -> Self {
        let (tx, ty) = ((mario.0 as u8).wrapping_sub(TARGET_OFFSET), mario.1 as u8);
        Self {
            x: Axis::new(START.0, tx),
            y: Axis::new(START.1, ty),
            radius: 0,
            shrinking: false,
            angles: ANGLES,
            tile_timer: 0,
            spin: 0,
            tile: 0,
            shown: [None; 8],
            done: false,
        }
    }

    /// One frame of 0x080304D0.
    pub fn step(&mut self) {
        self.shown = [None; 8];
        if self.done {
            return;
        }
        let grown = self.radius + RADIUS_STEP;
        if self.shrinking || grown >= RADIUS_TOP {
            self.shrinking = true;
            self.x.advance();
            self.y.advance();
            self.radius -= RADIUS_STEP;
            if self.radius == 0 {
                self.done = true;
                return;
            }
        } else {
            self.radius = grown;
        }
        let (t, carry) = self.tile_timer.overflowing_add(TILE_TICK);
        self.tile_timer = t;
        if carry {
            self.tile ^= 1;
        }
        let (s, carry) = self.spin.overflowing_add(SPIN);
        self.spin = s;
        for i in (0..8).rev() {
            self.angles[i] = (self.angles[i] + carry as u8) & 0x1F;
            self.shown[i] = self.place(self.angles[i]);
        }
    }

    /// 0x080302A4: a star's top-left, or `None` when it wraps off the screen.
    fn place(&self, angle: u8) -> Option<(u8, u8)> {
        let right = (angle & 0x18) < 0x10;
        let x = offset(self.x.pos, self.radius, SINE[(angle & 0xF) as usize], right)?;
        // The sprite's middle must not wrap either.
        if !fits(x.wrapping_add(8), self.x.pos, right) {
            return None;
        }
        let a = angle.wrapping_add(8);
        let y = offset(self.y.pos, self.radius, SINE[(a & 0xF) as usize], (angle.wrapping_sub(8) & 0x18) < 0x10)?;
        Some((x, y))
    }

    /// The stars to draw this frame: top-left screen position and cell.
    pub fn stars(&self) -> impl Iterator<Item = (i32, i32, usize)> + '_ {
        self.shown.iter().flatten().map(|&(x, y)| (x as i32, y as i32, self.tile))
    }
}

/// Centre +/- the star's offset; `None` if it wraps round the byte (0x08030240).
fn offset(centre: u8, radius: u8, sine: u8, plus: bool) -> Option<u8> {
    let d = scale(radius, sine);
    let v = if plus { centre.wrapping_add(d) } else { centre.wrapping_sub(d) };
    fits(v, centre, plus).then_some(v)
}

/// 0x08030240: moving away from the centre must not cross the byte's sign boundary the
/// wrong way.
fn fits(v: u8, centre: u8, plus: bool) -> bool {
    (v ^ centre) & 0x80 == 0 || if plus { v & 0x80 != 0 } else { v & 0x80 == 0 }
}

/// 0x080301A4 with 9 rounds: the high byte of `radius * sine * 2`.
fn scale(radius: u8, sine: u8) -> u8 {
    ((radius as u32 * sine as u32 * 2) >> 8) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Frames of the ROM's own intro (0x0800DE10 run in Unicorn, World 1, Mario on START at
    /// map (0x40, 0x20) = sprite (0x20, 0x27)): the stars' OAM after each update.
    #[test]
    fn matches_the_rom() {
        const H: Option<(u8, u8)> = None;
        let s = |x, y| Some((x, y));
        // (update number, OAM of the 8 stars, tile)
        type Frame = (usize, [Option<(u8, u8)>; 8], usize);
        let rom: [Frame; 4] = [
            (1, [s(0x6e, 0x46), s(0x70, 0x48), s(0x72, 0x46), s(0x74, 0x44), s(0x72, 0x42), s(0x70, 0x40), s(0x6e, 0x42), s(0x6c, 0x44)], 0),
            (22, [H, s(0x1b, 0x34), s(0x28, 0x74), s(0x60, 0x99), s(0xa0, 0x8c), s(0xc5, 0x54), s(0xb8, 0x14), H], 0),
            (28, [H, s(0x26, 0x08), s(0x10, 0x3a), s(0x26, 0x6c), s(0x58, 0x82), s(0x8a, 0x6c), s(0xa0, 0x3a), s(0x8a, 0x08)], 1),
            (45, [s(0x17, 0x28), s(0x16, 0x25), s(0x14, 0x25), s(0x11, 0x26), s(0x11, 0x28), s(0x12, 0x2b), s(0x14, 0x2b), s(0x17, 0x2a)], 1),
        ];
        let mut stars = MapStars::new((0x20, 0x27));
        let mut frame = 0;
        for (n, shown, tile) in rom {
            while frame < n {
                stars.step();
                frame += 1;
            }
            assert_eq!(stars.shown, shown, "update {n}");
            assert_eq!(stars.tile, tile, "tile at update {n}");
        }
        // One more frame: the circle has closed on Mario (+0x3C / +0x3D = 0x10, 0x27).
        stars.step();
        assert!(stars.done && stars.stars().next().is_none());
        assert_eq!((stars.x.pos, stars.y.pos), (0x10, 0x27));
    }
}
