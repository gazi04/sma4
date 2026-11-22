//! The airship's stormy sky (background scene 2, World 1's deck, level 0x205).
//!
//! * The scene's loader 0x080247C4 calls 0x08024650(2): an HBlank handler (0x080262E8)
//!   that sets BG2HOFS line by line from a table, and the per-frame routine 0x080263C0
//!   (0x08028C78 -> 0x0813F844[1]). That routine adds 0x40 to 0x03003BD4 and fills the
//!   table with five bands, 0x03003BD4 >> 8, >> 12, >> 11, >> 10 and >> 9, switching after
//!   the table's lines 0x87, 0x8D, 0x94 and 0xAA (0x08139DDC). The handler of line n sets
//!   line n + 1, so line 0 keeps the VBlank's value, camera x / 2 (0x03003758), and the
//!   bands start one line lower. The clouds drift left by themselves, whatever the camera.
//! * In the level's own mode (0x03003774 = 0x1A) the same routine runs the lightning
//!   (0x08008C5C): with no flash going, a random byte 0x03003879 of 5 or less starts one
//!   (sound 0x18); each frame of it writes step 0x03003BC9 of the colour tables 0x08132D9C
//!   (the backdrop) and 0x08132DA8 (palette 0, colours 8-15), steps 0-5, then 0x03003BC9 is
//!   0 again. Step 5 is the dark sky, so the deck starts in the level's own colours and
//!   turns dark with the first flash.
//! * The white clouds in front (BG3, scene mode 3) move by 0x03003BC0 / 16 px a frame
//!   (0x080008C8 in the VBlank: high nibble signed whole pixels, low nibble sixteenths
//!   kept in 0x03003BC1); each speed step of the autoscroll (0x080925B4) adds 2 to it, up to
//!   0x081711E8[world] (0x10 in World 1).

/// 0x03003BD4's step a frame.
const BAND_STEP: u16 = 0x40;
/// Shifts of 0x03003BD4 for the bands (0x03003BCA .. 0x03003BD2) and the table lines after
/// which the next band starts (0x08139DDC).
const BAND_SHIFTS: [u8; 5] = [8, 12, 11, 10, 9];
const BAND_ENDS: [u16; 4] = [0x87, 0x8D, 0x94, 0xAA];
/// The thunder's sound id (0x080E7D54(0x18), when a flash starts).
pub const SOUND_THUNDER: u8 = 0x18;
/// A flash starts with the random byte at most this (0x08008C5C).
const FLASH_CHANCE: u8 = 5;
/// Flash steps 0-5 (0x03003BC9 wraps after 5).
pub const FLASH_STEPS: u8 = 6;
/// The front clouds' speed cap by world (0x081711E8).
const FRONT_MAX: [u8; 9] = [0x10, 0x10, 0x10, 0x08, 0x10, 0x10, 0x10, 0x18, 0x08];
/// Scene 2's BG2 rows at the screen's top (VOFS 0x3F) are `sma4_assets::SKY_BACK_Y`.
pub const SCREEN_LINES: i32 = 0xA0;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Sky {
    /// 0x03003BD4.
    bands: u16,
    /// 0x03003BC9: the flash's next step (0: none going).
    flash: u8,
    /// The colour step last written (`None`: the level's own colours).
    pub look: Option<u8>,
    /// 0x03003BC0 (speed, 1/16 px a frame), 0x03003BC1 (the sixteenths), 0x03003760 /
    /// 0x03003BC4 (the front clouds' x).
    front_speed: u8,
    front_frac: u8,
    pub front_x: u16,
    /// Sound 0x18 (thunder) this frame.
    pub thunder: bool,
}

impl Sky {
    /// One frame of 0x080263C0. `rand` is 0x03003879; `level_mode`: the level's own mode
    /// (0x03003774 = 0x1A), when the lightning runs.
    pub fn step(&mut self, rand: u8, level_mode: bool) {
        self.thunder = false;
        self.bands = self.bands.wrapping_add(BAND_STEP);
        if level_mode {
            if self.flash == 0 {
                if rand > FLASH_CHANCE {
                    return;
                }
                self.thunder = true;
            }
            self.look = Some(self.flash);
            self.flash += 1;
            if self.flash >= FLASH_STEPS {
                self.flash = 0;
            }
        }
    }

    /// 0x080925B4: a speed step of the autoscroll speeds the front clouds up.
    pub fn autoscroll_step(&mut self, world: u8) {
        let max = FRONT_MAX.get(world as usize).copied().unwrap_or(0x10);
        self.front_speed = self.front_speed.wrapping_add(2);
        if self.front_speed >= max {
            self.front_speed &= max;
        }
    }

    /// 0x080008C8 in the VBlank: the front clouds move.
    pub fn move_front(&mut self) {
        let frac = (self.front_speed & 0xF) as u16 * 0x10 + self.front_frac as u16;
        self.front_frac = frac as u8;
        let whole = (self.front_speed >> 4) as i8;
        let whole = if whole > 7 { whole - 16 } else { whole };
        self.front_x = self.front_x.wrapping_add(frac >> 8).wrapping_add(whole as i16 as u16);
    }

    /// BG2's horizontal offset of screen line `line` (0-0x9F): line 0 the VBlank's camera
    /// x / 2, the others the band of the table line above.
    pub fn back_hofs(&self, line: i32, camera_x: i32) -> i32 {
        if line <= 0 {
            return (camera_x >> 1) & 0xFFFF;
        }
        let row = (line - 1) as u16;
        let band = BAND_ENDS.iter().filter(|&&end| end < row).count();
        (self.bands >> BAND_SHIFTS[band]) as i32
    }

    /// The screen lines (from, to) that share one offset: line 0, then each band.
    pub fn back_strips(&self, camera_x: i32) -> Vec<(i32, i32, i32)> {
        let mut out = vec![(0, 1, self.back_hofs(0, camera_x))];
        let mut from = 1;
        for &end in BAND_ENDS.iter().chain([&(SCREEN_LINES as u16)]) {
            let to = (end as i32 + 2).min(SCREEN_LINES);
            if from < to {
                out.push((from, to, self.back_hofs(from, camera_x)));
            }
            from = to;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// mGBA (World 1's deck from its first frame in mode 0x1A): 0x03003BD4 + 0x40 a frame;
    /// the random bytes 223, 111, 183, 91, 45, 22, 11 then 5 start a flash on the eighth
    /// frame, steps 0-5 one a frame, then the dark step 5 stays.
    #[test]
    fn the_lightning_flashes_six_steps_and_leaves_the_sky_dark() {
        let mut s = Sky::default();
        let mut looks = vec![];
        for r in [223, 111, 183, 91, 45, 22, 11, 5, 130, 65, 160, 208, 232, 116] {
            s.step(r, true);
            looks.push(s.look);
        }
        let none = [None; 7];
        assert_eq!(looks[..7], none);
        assert_eq!(looks[7..], [Some(0), Some(1), Some(2), Some(3), Some(4), Some(5), Some(5)]);
        assert_eq!(s.bands, 14 * 0x40);
    }

    /// mGBA: at 0x03003BD4 = 3904 the table holds 15 for lines 0-0x87, 0 after, 1 after
    /// 0x8D, 3 after 0x94; screen line 0 has camera x / 2.
    #[test]
    fn the_back_clouds_scroll_in_bands_by_line() {
        let s = Sky { bands: 3904, ..Default::default() };
        let got: Vec<_> = [0, 1, 0x88, 0x89, 0x8E, 0x8F, 0x95, 0x96, 0x9F].iter().map(|&l| s.back_hofs(l, 21)).collect();
        assert_eq!(got, [10, 15, 15, 0, 0, 1, 1, 3, 3]);
        assert_eq!(s.back_strips(21), [(0, 1, 10), (1, 0x89, 15), (0x89, 0x8F, 0), (0x8F, 0x96, 1), (0x96, 0xA0, 3)]);
    }

    /// mGBA: the speed goes 2, 4, ... 0x10 with the autoscroll's steps; at 2 the clouds
    /// move a pixel every 8 frames, at 0x10 one a frame.
    #[test]
    fn the_front_clouds_speed_up_to_a_pixel_a_frame() {
        let mut s = Sky::default();
        s.autoscroll_step(0);
        for _ in 0..8 {
            s.move_front();
        }
        assert_eq!(s.front_x, 1);
        for _ in 0..10 {
            s.autoscroll_step(0);
        }
        assert_eq!(s.front_speed, 0x10);
        let x = s.front_x;
        s.move_front();
        assert_eq!(s.front_x, x + 1);
    }
}
