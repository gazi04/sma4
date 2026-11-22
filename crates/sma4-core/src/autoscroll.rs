//! Autoscroll (see docs/re-notes.md, "Autoscroll"), as in World 1-4:
//!
//! * Enemy-list type 0x1A7 starts it when its column comes up (0x0804B768 -> 0x0804B0A4):
//!   the state block 0x03007848 + 0xBB0 (16 bytes) is cleared, kind +0xBBF = row >> 4,
//!   script +0xBC0 = row & 0xF (| 0x10 for kind 1), script index +0xBC1 = 0xFF, y +0xBB4 =
//!   the camera's y.
//! * Each frame the player is not busy (0x08092BF0: player +0x3E / +0x3B and 0x03007848 +
//!   0xBD1 clear) the step timer +0xBBE counts down and kind 0 runs (0x080926F4): x += x
//!   speed (0x0809216C), the player is kept on the screen (0x08092214), then the script
//!   step 0x080925B4: on even timer values y += y speed (0x08092130), and at 0 the next
//!   step (timer 4): a command's x / y speed steps 0x08170E14[(cmd >> 2) & 3] / [cmd & 3]
//!   for its duration, a sub-script repeated, or the end.
//! * The camera follows it (0x08099434 / 0x080994D0): x = +0xBB0 >> 8, y = +0xBB4 >> 8.
//!
//! Positions in 1/256 px, speeds in 1/16 px per frame (the ROM's units).

use sma4_assets::AutoscrollScripts;

/// Enemy-list type that starts an autoscroll.
pub const AUTOSCROLL: u16 = 0x1A7;
/// The player is pushed along when its frame is this close to the screen's edges
/// (0x08092214: screen x < 5 or > 0xDF).
pub const EDGE_LEFT: i32 = 5;
pub const EDGE_RIGHT: i32 = 0xDF;
/// Speed step every 4 frames (+0xBBE = 4).
const STEP_FRAMES: u8 = 4;
/// Forever (+0xBC3 = 0xFF).
const FOREVER: u8 = 0xFF;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Autoscroll {
    /// +0xBB0 / +0xBB4.
    pub x: i32,
    pub y: i32,
    /// +0xBB8 / +0xBBA.
    pub vx: i32,
    vy: i32,
    /// +0xBC0, +0xBC1, +0xBBD (steps left of the command).
    script: u8,
    index: u8,
    left: u8,
    /// Sub-script +0xBC2, its repeats left +0xBC3 (0: none), its entry +0xBC4.
    sub: u8,
    repeat: u8,
    sub_index: u8,
    /// +0xBBE.
    timer: u8,
    /// A speed step ran this frame (0x080925B4; scene 2 speeds its front clouds up then).
    pub stepped: bool,
}

impl Autoscroll {
    /// 0x0804B0A4 for the list entry's row; kinds 0 and 1 (handler 0x080926F4, scripts
    /// 0-15 / 16-31) are ported.
    pub fn start(row: u8, camera_y: i32) -> Option<Self> {
        let kind = row >> 4;
        (kind < 2).then_some(Self {
            x: 0,
            y: camera_y << 8,
            vx: 0,
            vy: 0,
            script: row & 0xF | if kind == 1 { 0x10 } else { 0 },
            index: 0xFF,
            left: 0,
            sub: 0,
            repeat: 0,
            sub_index: 0,
            timer: 0,
            stepped: false,
        })
    }

    /// Camera position, px.
    pub fn camera(&self) -> (i32, i32) {
        (self.x >> 8 & 0xFFFF, self.y >> 8)
    }

    /// One frame (0x08092BF0 -> 0x080926F4).
    pub fn step(&mut self, scripts: &AutoscrollScripts) {
        self.stepped = false;
        self.timer = self.timer.saturating_sub(1);
        self.x += self.vx * 16;
        // 0x080925B4.
        if self.timer & 1 != 0 {
            return;
        }
        self.y += self.vy * 16;
        if self.timer >> 1 != 0 || (self.left == 0 && self.advance(scripts)) {
            return;
        }
        self.left = self.left.wrapping_sub(1);
        let Some(script) = scripts.scripts.get(self.script as usize) else { return };
        let mut cmd = script.commands.get(self.index as usize).copied().unwrap_or(0);
        if self.repeat != 0
            && let Some(&c) = scripts.subs.get(self.sub as usize).and_then(|s| s.commands.get(self.sub_index as usize))
            && (c as i8) >= 0
        {
            cmd = c;
        }
        self.vx += scripts.steps[(cmd >> 2 & 3) as usize] as i32;
        self.vy += scripts.steps[(cmd & 3) as usize] as i32;
        self.timer = STEP_FRAMES;
        self.stepped = true;
    }

    /// 0x080923F0: the next command, sub-script entry or repeat; true once the script has
    /// ended (the camera stops: x speed 0; a script with nothing to loop also stops y and
    /// drops the x fraction).
    fn advance(&mut self, scripts: &AutoscrollScripts) -> bool {
        let sub_len = |s: &Self| scripts.subs.get(s.sub as usize).map_or(0, |s| s.commands.len());
        if self.repeat != 0 {
            if self.sub_index.wrapping_add(1) as usize != sub_len(self) {
                return self.next_sub_entry(scripts);
            }
            self.sub_index = 0xFF;
            if self.repeat != FOREVER {
                self.repeat -= 1;
                if self.repeat != 0 {
                    return self.next_sub_entry(scripts);
                }
            } else {
                return self.next_sub_entry(scripts);
            }
        }
        let Some(script) = scripts.scripts.get(self.script as usize) else { return true };
        let next = self.index.wrapping_add(1);
        if next as usize == script.commands.len() {
            self.vx = 0;
            if script.then == 0 {
                self.vy = 0;
                self.x &= !0xFF;
                return true;
            }
            self.sub = script.then - 1;
            self.repeat = FOREVER;
        } else {
            self.index = next;
            let (cmd, dur) = (script.commands[next as usize], script.durations[next as usize]);
            if cmd >> 4 == 0 {
                self.left = dur;
                return false;
            }
            self.sub = (cmd >> 4) - 1;
            self.repeat = dur;
        }
        self.sub_index = 0xFF;
        self.next_sub_entry(scripts)
    }

    fn next_sub_entry(&mut self, scripts: &AutoscrollScripts) -> bool {
        self.sub_index = self.sub_index.wrapping_add(1);
        self.left = scripts
            .subs
            .get(self.sub as usize)
            .and_then(|s| s.durations.get(self.sub_index as usize))
            .copied()
            .unwrap_or(0);
        false
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use sma4_assets::ScrollScript;

    /// World 1-4's script 0 and the sub-scripts it uses, as in the ROM.
    pub(crate) fn scripts() -> AutoscrollScripts {
        AutoscrollScripts {
            steps: [0, 1, -1, -1],
            scripts: vec![ScrollScript { commands: vec![0x04, 0x10, 0x00], durations: vec![0x08, 0x03, 0x86], then: 0 }],
            subs: vec![ScrollScript { commands: vec![0x00], durations: vec![0xFF], then: 0 }],
        }
    }

    #[test]
    fn world_1_4_speeds_up_to_half_a_pixel_a_frame_then_stops_at_the_end() {
        let s = scripts();
        let mut a = Autoscroll::start(0, 0xF0).unwrap();
        let mut frames = 0;
        let mut last = -1;
        // Speeds up by 1/16 px per frame every 4 frames, eight times.
        for _ in 0..40 {
            a.step(&s);
            frames += 1;
        }
        assert_eq!(a.vx, 8);
        while a.vx != 0 {
            a.step(&s);
            frames += 1;
            assert!(frames < 10_000);
        }
        assert_eq!(a.camera().1, 0xF0);
        for _ in 0..100 {
            a.step(&s);
            let x = a.camera().0;
            assert!(x >= last);
            last = x;
        }
        // 8 steps speeding up, 3 x 255 + 0x86 steps at 0.5 px a frame.
        assert_eq!(frames, 4 * (8 + 3 * 255 + 0x86) + 1);
        assert_eq!(a.camera().0, (4 * (1..=8).sum::<i32>() * 16 + 4 * (3 * 255 + 0x86) * 8 * 16) >> 8);
    }
}
