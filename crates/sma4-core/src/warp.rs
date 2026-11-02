//! The Warp Whistle on the map (item 12, 0x08006850; then map state +0x16 through
//! 0x0813255C; see docs/re-notes.md, "Warp Whistle"). Timings recorded in mGBA, World 1,
//! Mario at (0x40, 0x20) with the camera at 8:
//!
//! * 0x08006850: the whirlwind comes from the right when Mario's screen x + 8 is below
//!   0x80 (+0x15 = 1: x 0x0813AD52[1] = 0xF0, 2 px left a frame), else from the left
//!   (x 0, 2 px right); it flies along Mario's row (+0x11). Music 0x0E.
//! * +0x16 = 1 (0x08007760): the map waits 44 frames (0x03003850), the HBlank wave handler
//!   (0x08007664) goes in.
//! * 2 (0x08007848): the whirlwind flies across (x + 0x0813AD56[side] a frame); where it
//!   meets Mario he is gone (0x030037A8 = 0xF0); 128 frames later it is back at 0xF0
//!   (0x0813AD5A), off the screen. Then the wave grows: +0x69 + 1 on frames with
//!   0x03003AE8 & 3 == 0, to 0x40; then World 9 (0x03002A69 = 8, the old world in
//!   0x03003CB7).
//! * 3 (0x08007950) and the map reload (mode-1 sub-state 0x12 for 24 frames): 39 frames.
//!   Mario's new spot: x 0x0813AD65[old world], y 0x0813AD5C[old world] (World 1: 0x40,
//!   0x50).
//! * 4 (0x08007A04): the wave dies down (+0x69 - 1 on the same frames), then the whirlwind
//!   flies across again from 0xF0 and drops Mario on his spot as it passes it; at 0xF0
//!   again it is over.
//!
//! The wave (recorded line offsets, 0x020033A0): line L is shifted by about
//! (2 * amp - 1) * sin(2 pi (3L + phase) / 256), the phase going up 1 a frame.

/// Frames before the whirlwind moves: +0x16 = 1 for 44, then the frame +0x16 becomes 2.
pub const WAIT: u16 = 45;
/// The whirlwind's start x by side, step, and the x it ends at (off the screen).
const START_X: [i32; 2] = [0x00, 0xF0];
const STEP: [i32; 2] = [2, -2];
const END_X: i32 = 0xF0;
/// The wave's peak amplitude.
pub const PEAK: u8 = 0x40;
/// From World 9 (+0x16 = 3) to the wave shrinking (+0x16 = 4), the map reloading.
pub const RELOAD: u16 = 39;
/// The black part of the reload (mode-1 sub-state 0x12: frames 14 to 38).
const RELOAD_BLACK: std::ops::Range<u16> = 14..38;
/// Mario's spot in World 9 by the world he left (0x0813AD5C / 0x0813AD65), World 1.
pub const LANDING: (u8, u16) = (0x50, 0x40);
/// World 9's index (0x03002A69).
pub const WARP_ZONE: u8 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Wait(u16),
    /// The whirlwind takes Mario away.
    Away,
    Grow,
    /// World 9 is loading (frames so far).
    Reload(u16),
    Shrink,
    /// The whirlwind brings Mario in.
    Bring,
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Warp {
    pub phase: Phase,
    /// 0 from the left, 1 from the right (+0x15).
    side: usize,
    /// The whirlwind's screen x (+0x13, a byte) and its frames so far (+0x14).
    x: i32,
    frames: u16,
    /// Mario's screen x (to pick him up), and his map y (the whirlwind's row, +0x11).
    mario_x: i32,
    pub row: u8,
    /// +0x69 and the wave's phase.
    pub amp: u8,
    wave_phase: u8,
    /// Mario is carried (0x030037A8 = 0xF0) or not there yet.
    pub mario_hidden: bool,
}

/// What the game must do this frame.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Event {
    /// Load World 9 now.
    pub switch_world: bool,
    /// Mario appears on his World 9 spot.
    pub landed: bool,
    pub done: bool,
}

impl Warp {
    /// 0x08006850 with Mario at screen x `mario_x` on map row y `row`.
    pub fn start(mario_x: i32, row: u8) -> Self {
        let side = usize::from(((mario_x + 8) as i8) >= 0);
        Self {
            phase: Phase::Wait(0),
            side,
            x: START_X[side],
            frames: 0,
            mario_x,
            row,
            amp: 0,
            wave_phase: 0,
            mario_hidden: false,
        }
    }

    /// The whirlwind's screen x and flip (coming from the right) while it shows.
    pub fn whirlwind(&self) -> Option<(i32, bool)> {
        matches!(self.phase, Phase::Away | Phase::Bring).then_some((self.x, self.side == 1))
    }

    /// Its animation frame (+0x14 bit 4).
    pub fn whirlwind_frame(&self) -> usize {
        usize::from(self.frames & 0x10 != 0)
    }

    /// The wave's shift of screen line `line`, px.
    pub fn shift(&self, line: i32) -> i32 {
        if self.amp == 0 {
            return 0;
        }
        let a = 2.0 * self.amp as f64 - 1.0;
        let t = (3 * line + self.wave_phase as i32) as f64 * std::f64::consts::TAU / 256.0;
        (a * t.sin()).round() as i32
    }

    /// The screen is black (the reload).
    pub fn black(&self) -> bool {
        matches!(self.phase, Phase::Reload(k) if RELOAD_BLACK.contains(&k))
    }

    /// One frame; `frame` is the frame counter (0x03003AE8).
    pub fn step(&mut self, frame: u32) -> Event {
        let mut ev = Event::default();
        if self.amp != 0 {
            self.wave_phase = self.wave_phase.wrapping_add(1);
        }
        let tick = frame & 3 == 0;
        match self.phase {
            Phase::Wait(k) => self.phase = if k + 1 >= WAIT { Phase::Away } else { Phase::Wait(k + 1) },
            Phase::Away | Phase::Bring => {
                self.frames += 1;
                self.x = (self.x + STEP[self.side]) & 0xFF;
                let away = self.phase == Phase::Away;
                if away && self.x == self.mario_x {
                    self.mario_hidden = true;
                }
                if !away && self.x == LANDING.1 as i32 {
                    self.mario_hidden = false;
                    ev.landed = true;
                }
                if self.x == END_X {
                    self.phase = if away { Phase::Grow } else { Phase::Done };
                    ev.done = !away;
                }
            }
            Phase::Grow => {
                if tick {
                    self.amp += 1;
                }
                if self.amp == PEAK {
                    self.phase = Phase::Reload(0);
                    ev.switch_world = true;
                }
            }
            Phase::Reload(k) => {
                if k + 1 >= RELOAD {
                    self.phase = Phase::Shrink;
                    self.row = LANDING.0;
                    self.x = START_X[self.side];
                    self.frames = 0;
                } else {
                    self.phase = Phase::Reload(k + 1);
                }
            }
            Phase::Shrink => {
                if tick {
                    self.amp -= 1;
                }
                if self.amp == 0 {
                    self.phase = Phase::Bring;
                }
            }
            Phase::Done => ev.done = true,
        }
        ev
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// mGBA, World 1 (Mario at screen x 24, the camera at 8; the whistle used at frame 19):
    /// the whirlwind leaves 0xF0 at frame 65 and meets Mario at 172, is off the screen at
    /// 192, World 9 comes at 447.
    #[test]
    fn whistle_timeline() {
        let mut w = Warp::start(24, 0x40);
        assert_eq!(w.whirlwind(), None);
        let (mut f, mut met, mut gone, mut switch) = (19u32, None, None, None);
        while switch.is_none() {
            f += 1;
            let ev = w.step(f);
            if w.mario_hidden && met.is_none() {
                met = Some(f);
            }
            if w.phase == Phase::Grow && gone.is_none() {
                gone = Some(f);
            }
            if ev.switch_world {
                switch = Some(f);
            }
            assert!(f < 1000);
        }
        assert_eq!(w.whirlwind(), None);
        assert_eq!(met, Some(65 + 107));
        assert_eq!(gone, Some(192));
        assert!((444..=450).contains(&switch.unwrap()), "{switch:?}");
        let (mut landed, mut done) = (None, None);
        while done.is_none() {
            f += 1;
            let ev = w.step(f);
            if ev.landed {
                landed = Some(f);
            }
            if ev.done {
                done = Some(f);
            }
        }
        assert!(landed.unwrap() < done.unwrap());
        assert!(!w.mario_hidden);
    }

    #[test]
    fn the_wave_peaks_at_twice_the_amplitude() {
        let mut w = Warp::start(24, 0x40);
        w.amp = 40;
        let max = (0..160).map(|l| w.shift(l).abs()).max().unwrap();
        assert_eq!(max, 79);
    }
}
