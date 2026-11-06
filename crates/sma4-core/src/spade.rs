//! The spade panel's bonus game: Toad's room, then three rows of pictures to line up for
//! extra lives (see docs/re-notes.md, "Spade panel").
//!
//! * The map spot (tileset 15, enemy word 0x8001: game kind 0x030038EC = 1) loads level
//!   0x240 (0x08135B20[0]), whose loader 0x08015F10 builds Toad's room and starts mode-1
//!   sub-state 0x23 (the level loop) then 0x24 (0x08033618: input cleared, 0x08033450).
//! * Room (0x08033450, state 0x03003904[0] through 0x081440C8): Toad drawn every frame
//!   (0x08033418); state 0 (0x08032FC8) draws one box row a frame, then starts the message
//!   0x08244318[language][min(count, 3)] (state 7, 0x080330B8: one byte every 4 frames,
//!   0x0300384D; the end byte -> state 8). From state 7 on, A (0x03003AEC bit 0) with Mario
//!   standing at the door -> state 9, 0x03003AE7 = 1.
//! * Next frame 0x08033618 moves on: sub-state 0x25 (0x0800373C) takes 4 off every palette
//!   channel every other frame, 8 times; 0x26 (0x0803377C) loads the slots with the screen
//!   black (BLDY 0x10); 0x27 (0x08033A00) brightens it a step a frame; 0x28 (0x0803470C)
//!   runs the game through 0x08144118[0x030038EE].
//! * Slots, state 0 (0x08033A2C): rows at 0, speeds 0x70 / 0x90 / 0x7F (signed, x16 per
//!   frame in 1/256 px). State 1 (0x0803426C): A stops the next row after a random delay
//!   ((0x03003879 & 0xF) | 0x20, (0x0300387A & 0x1F) | 0x20, (0x0300387B & 0x3F) | 0x40);
//!   each row (bottom first) runs 0x08144100[its state]: 0 spin, 1 delay, 2 slow down by 2
//!   until |speed| < 0x40, 3 snap to the picture when within 4 px of it, 4 wobble 0x12
//!   frames (1 px, sign by timer bit 2) and snap again: the picture is (x >> 7) & 3, 5 done.
//! * The bottom row stopping decides (0x08033F30): the other rows must show one of the top
//!   row's pictures (0x08143E0C pairs), or the streak count (0x03002CD8 bits 2-5) is 3 or
//!   more. A win gives 0x08143E14[count * 4 + top picture] lives; the count goes up (three
//!   wins in a row: count 0x08143E3F[0x03002CD9], the bonus points the prizes add up,
//!   0x08143E34); a loss clears both.
//! * Win: state 4 waits 0x60 frames, state 5 (0x0803446C) raises the prize sprite from y
//!   0xF0 by 4 a frame until below 0x48, then adds a life every 0x20 frames (frame counter
//!   0x03003AE8) and 0xA0 frames after each (0x50 at first); the BG2 icons sparkle
//!   meanwhile (0x080346CC). Then state 2 (0x080343A0) fades out (BLDY + 1 every other
//!   frame) and state 3 (0x080343FC) leaves through the level end (sub-state 0x1C).

use sma4_assets::{SlotWin, SpadeAsset};

use crate::input::Buttons;
use crate::rng::Rng;

/// Map tileset of the spade spots, and the game kind of the World 1 spade (0x030038EC).
pub const TILESET: u8 = 15;
pub const KIND_SLOTS: u16 = 1;
/// Mario's frame in Toad's room (0x08015F10 / 0x08033450: x 0x4400, y 0x16000): he stands
/// at the door, facing right, his 32x32 sprite at OAM (0x34, 0x59) (read from the running
/// ROM: 9 px under frame y - BG1's 0x110, the room's floor being drawn 8 px under his feet).
pub const HOST_MARIO: (i32, i32) = (0x44, 0x160);
pub const HOST_MARIO_OAM: (i32, i32) = (0x34, 0x59);

const BOX_ROWS: u8 = sma4_assets::SPADE_BOX_ROWS as u8;
/// 0x0300384D: a byte every 4 frames.
const TEXT_STEP: u8 = 4;
const NEWLINE: u8 = 0xF0;
const SKIP: u8 = 0xFE;
const END: u8 = 0xFF;
/// 0x0800373C: palette steps and the channel amount per step.
pub const FADE_STEPS: u8 = 8;
pub const FADE_STEP: u8 = 4;
/// BLDY at full black.
pub const BLACK: u8 = 0x10;
/// The way in (mGBA, World 1's spade, A pressed at frame 0): the map's iris closes, then
/// sub-states 0x14 (15 frames), 0x15 (1) and 0x16 (15, BLDY 0x10) keep the screen black,
/// and 0x23 starts at frame 75. It grows the iris radius 0x03003B64 by 8 a frame from 0
/// for 32 frames (0x03003918 = 0x38 ends it), centred on the screen. The window is drawn
/// with a lag: at the 0x23 frame k it is a circle of radius 8k - 17 round (120, 80)
/// (nothing at first, the whole screen from k = 18). The port's map hands over at frame
/// 43, so `OPEN_BLACK` frames of black make the iris start at 75.
pub const OPEN_BLACK: u8 = 32;
pub const OPEN_IRIS: u8 = 32;
const IRIS_STEP: i32 = 8;
const IRIS_LAG: i32 = 17;
pub const IRIS_CENTRE: (i32, i32) = (120, 80);

/// Row speeds at the start (0x08033A2C).
const SPEEDS: [u8; 3] = [0x70, 0x90, 0x7F];
/// Random delay before a row slows down: mask, then or (0x0803426C).
const DELAYS: [(u8, u8); 3] = [(0x0F, 0x20), (0x1F, 0x20), (0x3F, 0x40)];
/// Wobble once snapped (0x08033D30), and the wait after the last row (0x08033F30).
const WOBBLE: u8 = 0x12;
const RESULT_WAIT: u8 = 0x50;
/// Pictures that match the top row's, by its picture (0x08143E0C).
const SAME: [[u8; 2]; 4] = [[0, 2], [1, 1], [2, 0], [3, 3]];
/// Lives won by streak count * 4 + picture (0x08143E14).
const PRIZES: [u8; 32] = [
    2, 3, 2, 5, 2, 3, 2, 7, 2, 3, 2, 10, 2, 2, 2, 2, 3, 3, 3, 3, 5, 5, 5, 5, 7, 7, 7, 7, 10, 10, 10, 10,
];
/// Bonus points by prize (0x08143E34), and the streak count they make after three wins
/// (0x08143E3F).
const BONUS: [u8; 11] = [0, 0, 1, 2, 0, 3, 0, 3, 0, 0, 3];
const BONUS_COUNT: [u8; 10] = [3, 3, 3, 3, 4, 4, 5, 5, 6, 7];
/// Win: wait (0x03003BAF), then the sprite from this y (0x030038EF), up 4 a frame to below
/// 0x48 (0x0803446C).
const WIN_WAIT: u8 = 0x60;
const PRIZE_START_Y: u8 = 0xF0;
const PRIZE_STOP_Y: u8 = 0x48;
const PRIZE_RISE: u8 = 4;
const PRIZE_HOLD: u8 = 0x50;
const LIFE_HOLD: u8 = 0xA0;
const MAX_LIVES: u16 = 999;
/// Blank BG2 tile (0x08033E54).
const BLANK: u16 = 0xFD;
/// BG2 map width.
const BG2_W: u16 = 0x20;

/// The streak (0x03002CD8 bits 2-5: wins in a row, picks the slots; 0x03002CD9: bonus
/// points). Kept across games; a game over clears it (0x08012458).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Streak {
    pub count: u8,
    pub bonus: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Room {
    /// State 0: box rows drawn.
    Box(u8),
    /// State 7.
    Text,
    /// State 8.
    Idle,
    /// State 9: A pressed.
    Leaving,
}

/// Mode-1 sub-states 0x16 and 0x23-0x28.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// The way in (frames so far): sub-state 0x16 (the screen black, `OPEN_BLACK` frames),
    /// then 0x23, the level loop, opening an iris on Toad's room (`OPEN_IRIS` frames).
    Opening(u8),
    Room,
    /// 0x25: palette steps done, and the step counter 0x03003784; after the 8th, 0x26 loads
    /// the slots.
    RoomFade(u8, i8),
    /// 0x27: BLDY.
    FadeIn(u8),
    Slots,
}

/// How the screen is darkened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shade {
    /// Every palette channel less this (of 31; 0x0800373C).
    Subtract(u8),
    /// BLDY: colours scaled by (16 - y) / 16.
    Darken(u8),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Reel {
    /// 0x030038EF[i].
    pub state: u8,
    /// 0x030038F2[i], signed.
    speed: u8,
    /// 0x030038F5[i].
    timer: u8,
    /// x in 1/256 px (bytes 0x030038FE[i], 0x030038FB[i], 0x030038F8[i]).
    pub pos: u32,
    /// 0x03003901[i].
    pub picture: u8,
}

impl Reel {
    /// BG1HOFS for the row's lines (VBlank 0x08001C9C: x + 8).
    pub fn scroll(&self) -> u16 {
        ((self.pos >> 8) as u16).wrapping_add(8)
    }

    /// 0x08033AE4.
    fn spin(&mut self) {
        self.pos = (self.pos as i32 + self.speed as i8 as i32 * 16) as u32 & 0xFF_FFFF;
    }

    /// 0x08033D30 / 0x08033F30: x + 0x20 px down to a picture's edge (the low byte stays).
    fn snap(&mut self) {
        let px = ((self.pos >> 8) + 0x20) & 0xFF80;
        self.pos = px << 8 | self.pos & 0xFF;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slots {
    /// 0x030038EE.
    pub state: u8,
    /// Top, middle, bottom.
    pub reels: [Reel; 3],
    /// The streak count when the game started: it picks the pictures.
    pub count: u8,
    /// Lives won (0x03003904[1]).
    pub prize: u8,
    /// The prize sprite's y (0x030038EF after a win), drawn this frame.
    pub prize_y: u8,
    pub prize_shown: bool,
    /// Lives still to add (0x030038F0) and the wait (0x030038F5).
    lives_left: u8,
    hold: u8,
    /// 0x03003BAF.
    wait: u8,
    /// Sparkle counter and phase (0x03003BB7 / BB8).
    sparkle: u8,
    sparkle_phase: u8,
    /// Fade out: BLDY (0x03002DB0), and its first frame (0x030038EF = 0) done.
    pub bldy: u8,
    fading: bool,
    /// BG2 map (32x32), changed by a win; the generation counts the changes.
    pub bg2: Vec<u16>,
    pub bg2_generation: u32,
}

impl Slots {
    pub fn new(count: u8, data: &SpadeAsset) -> Self {
        let bg2 = data.bg2.get(count as usize).cloned().unwrap_or_else(|| vec![0; 0x400]);
        Self {
            state: 0,
            reels: [Reel::default(); 3],
            count,
            prize: 0,
            prize_y: 0,
            prize_shown: false,
            lives_left: 0,
            hold: 0,
            wait: 0,
            sparkle: 0,
            sparkle_phase: 0,
            bldy: 0,
            fading: false,
            bg2,
            bg2_generation: 0,
        }
    }

    fn put(&mut self, at: u16, entries: &[u16]) {
        for (i, &e) in entries.iter().enumerate() {
            if let Some(c) = self.bg2.get_mut(at as usize + i) {
                *c = e;
            }
        }
    }

    /// 0x08033DBC: a 2x2 entry block at each map index of `list`.
    fn blocks(&mut self, list: &[u16], e: [u16; 4]) {
        for &at in list {
            self.put(at, &e[..2]);
            self.put(at + BG2_W, &e[2..]);
        }
        self.bg2_generation += 1;
    }

    /// One frame of 0x0803470C. Returns true once the game is over (state 3).
    pub fn step(&mut self, pressed: Buttons, rng: &Rng, frame: u32, streak: &mut Streak, lives: &mut u16, data: &SpadeAsset) -> bool {
        self.prize_shown = false;
        let win = data.wins.iter().find(|w| w.prize == self.prize);
        // 0x080346CC: the icons sparkle from the win on.
        if self.state > 3
            && let Some(w) = win
        {
            self.sparkle = self.sparkle.wrapping_add(1);
            self.sparkle_step(w, data);
        }
        match self.state {
            0 => {
                for (r, &s) in self.reels.iter_mut().zip(&SPEEDS) {
                    *r = Reel { speed: s, ..*r };
                    r.state = 0;
                    r.pos = 0;
                }
                self.prize = 0;
                self.state = 1;
            }
            1 => self.play(pressed, rng, streak, data),
            2 => {
                // 0x080343A0: one frame, then BLDY + 1 on even frames up to 0x10.
                if !self.fading {
                    self.fading = true;
                } else if frame & 1 == 0 {
                    self.bldy += 1;
                    if self.bldy >= BLACK {
                        self.state = 3;
                    }
                }
            }
            3 => return true,
            4 => {
                if self.wait == 0 {
                    self.state = 5;
                } else {
                    self.wait -= 1;
                }
            }
            _ => self.raise_prize(frame, lives),
        }
        false
    }

    /// 0x080341B0.
    fn fade_out(&mut self) {
        self.state = 2;
        self.fading = false;
        self.bldy = 0;
    }

    /// State 1 (0x0803426C).
    fn play(&mut self, pressed: Buttons, rng: &Rng, streak: &mut Streak, data: &SpadeAsset) {
        if pressed.contains(Buttons::A) {
            let [r0, r1, r2] = &self.reels;
            let next = if r0.state == 0 {
                Some(0)
            } else if r1.state == 0 && r0.state >= 2 {
                Some(1)
            } else if r2.state == 0 && r1.state >= 2 {
                Some(2)
            } else {
                None
            };
            if let Some(i) = next {
                let (mask, or) = DELAYS[i];
                self.reels[i].state = 1;
                self.reels[i].timer = rng.byte(i) & mask | or;
            }
        }
        for i in (0..3).rev() {
            let r = &mut self.reels[i];
            r.timer = r.timer.saturating_sub(1);
            if self.reel_step(i, streak, data) {
                return;
            }
        }
    }

    /// 0x08144100[state] for row `i`. Returns true when the game moved on.
    fn reel_step(&mut self, i: usize, streak: &mut Streak, data: &SpadeAsset) -> bool {
        let r = &mut self.reels[i];
        match r.state {
            0 => r.spin(),
            1 => {
                if r.timer == 0 {
                    r.state = 2;
                }
                r.spin();
            }
            2 => {
                if r.speed.wrapping_add(0x3F) < 0x7F {
                    r.state = 3;
                } else {
                    if r.speed & 0x80 != 0 {
                        r.speed = r.speed.wrapping_add(4);
                    }
                    r.speed = r.speed.wrapping_sub(2);
                }
                r.spin();
            }
            3 => {
                if (r.pos >> 8) & 0x7C == 0 {
                    r.timer = WOBBLE;
                    r.snap();
                    r.state = 4;
                }
                r.spin();
            }
            4 => {
                if r.timer != 0 {
                    r.speed = if r.timer & 4 != 0 { 0xF0 } else { 0x10 };
                    r.spin();
                    return false;
                }
                r.snap();
                r.picture = ((r.pos >> 15) & 3) as u8;
                if i != 2 {
                    r.state = 5;
                    return false;
                }
                r.timer = RESULT_WAIT;
                let same = SAME[self.reels[0].picture as usize & 3];
                let lined_up = self.reels[1..].iter().all(|r| same.contains(&r.picture));
                if lined_up || streak.count >= 3 {
                    self.win(streak, data);
                    return true;
                }
                self.reels[2].state = 5;
            }
            _ => {
                // 0x080341E4: the bottom row's wait over, a loss fades out.
                if i == 2 && r.timer == 0 {
                    *streak = Streak::default();
                    self.fade_out();
                    return true;
                }
            }
        }
        false
    }

    /// 0x08033F30's win: the prize, the signs (0x08033E54), the streak.
    fn win(&mut self, streak: &mut Streak, data: &SpadeAsset) {
        self.prize = PRIZES[(streak.count as usize * 4 + self.reels[0].picture as usize) & 31];
        if let Some(w) = data.wins.iter().find(|w| w.prize == self.prize) {
            for &at in &w.sign_at {
                self.put(at, &w.sign);
            }
            self.blocks(&w.sparkle_at[0], [BLANK; 4]);
            self.blocks(&w.sparkle_at[1], [BLANK; 4]);
        }
        streak.bonus = streak.bonus.wrapping_add(BONUS.get(self.prize as usize).copied().unwrap_or(0));
        streak.count = (streak.count + 1) & 0xF;
        if streak.count > 3 {
            *streak = Streak::default();
        }
        if streak.count > 2 {
            streak.count = BONUS_COUNT.get(streak.bonus as usize).copied().unwrap_or(0) & 0xF;
        }
        self.wait = WIN_WAIT;
        self.state = 4;
        self.prize_y = PRIZE_START_Y;
        self.hold = 0;
    }

    /// 0x08034578 (first sparkle) / 0x08034650 (then round the icon groups).
    fn sparkle_step(&mut self, w: &SlotWin, data: &SpadeAsset) {
        let t = self.sparkle;
        if self.sparkle_phase == 0 {
            if t & 7 == 0 {
                let e = w.sparkles[(t as usize & 0x18) >> 3];
                self.blocks(&w.sparkle_at[0], e);
                self.blocks(&w.sparkle_at[1], e);
            }
            if t == 0x18 {
                self.blocks(&w.end_at, [BLANK; 4]);
                self.sparkle = 0;
                self.sparkle_phase = 1;
            }
        } else if t & 7 == 0 {
            let e = w.sparkles[(t as usize & 0x18) >> 3];
            self.blocks(&data.cycle[(t as usize & 0x60) >> 5], e);
        }
    }

    /// State 5 (0x0803446C).
    fn raise_prize(&mut self, frame: u32, lives: &mut u16) {
        if self.hold == 0 {
            self.prize_y = self.prize_y.wrapping_sub(PRIZE_RISE);
            if self.prize_y < PRIZE_STOP_Y {
                self.lives_left = self.prize;
                self.hold = PRIZE_HOLD;
            }
        } else {
            self.hold -= 1;
            if self.hold == 0 {
                self.fade_out();
                return;
            }
            if frame & 0x1F == 0 && self.lives_left != 0 {
                self.hold = LIFE_HOLD;
                self.lives_left -= 1;
                if *lives < MAX_LIVES {
                    *lives += 1;
                }
            }
        }
        self.prize_shown = true;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spade {
    pub phase: Phase,
    room: Room,
    /// 0x0300384D.
    text_timer: u8,
    message: Vec<u8>,
    printed: usize,
    pub slots: Option<Slots>,
    data: SpadeAsset,
}

impl Spade {
    /// Toad's room, his message picked by the streak count.
    pub fn new(data: SpadeAsset, streak: Streak) -> Self {
        let k = (streak.count as usize).min(sma4_assets::SPADE_MESSAGES - 1);
        let message = data.messages[k].clone();
        Self { phase: Phase::Opening(0), room: Room::Box(0), text_timer: TEXT_STEP, message, printed: 0, slots: None, data }
    }

    /// While coming in: the radius of the window on the room (negative: all black).
    pub fn iris(&self) -> Option<i32> {
        match self.phase {
            Phase::Opening(k) if k < OPEN_BLACK => Some(-1),
            Phase::Opening(k) => Some(IRIS_STEP * (k - OPEN_BLACK) as i32 - IRIS_LAG),
            _ => None,
        }
    }

    /// The message for this visit (`spade_host_text(k)`).
    pub fn message_index(streak: Streak) -> usize {
        (streak.count as usize).min(sma4_assets::SPADE_MESSAGES - 1)
    }

    /// Message bytes read so far (0x03003BAC).
    pub fn printed(&self) -> usize {
        self.printed
    }

    /// Box rows drawn.
    pub fn box_rows(&self) -> u32 {
        match self.room {
            Room::Box(n) => n as u32,
            _ => BOX_ROWS as u32,
        }
    }

    /// Cells of the message printed so far, (column, row) from `SPADE_TEXT_AT`.
    pub fn text_cells(&self) -> Vec<(u32, u32)> {
        let (mut col, mut row) = (0, 0);
        let mut out = Vec::new();
        for &b in &self.message[..self.printed] {
            match b {
                NEWLINE => (col, row) = (0, row + 1),
                SKIP => col += 1,
                END => break,
                _ => {
                    out.push((col, row));
                    col += 1;
                }
            }
        }
        out
    }

    pub fn shade(&self) -> Option<Shade> {
        match self.phase {
            Phase::Opening(_) | Phase::Room => None,
            Phase::RoomFade(steps, _) => Some(Shade::Subtract(steps * FADE_STEP)),
            Phase::FadeIn(y) => Some(Shade::Darken(y)),
            Phase::Slots => self.slots.as_ref().and_then(|s| (s.state == 2 || s.state == 3).then_some(Shade::Darken(s.bldy))),
        }
    }

    /// One frame. Returns true when the game is over (back to the map, the spot cleared).
    pub fn step(&mut self, pressed: Buttons, rng: &Rng, frame: u32, streak: &mut Streak, lives: &mut u16) -> bool {
        match self.phase {
            Phase::Opening(k) => {
                self.phase = if k + 1 >= OPEN_BLACK + OPEN_IRIS { Phase::Room } else { Phase::Opening(k + 1) };
            }
            Phase::Room => self.room_step(pressed),
            // 0x26 (0x0803377C): the slots, the screen black.
            Phase::RoomFade(steps, _) if steps >= FADE_STEPS => {
                self.slots = Some(Slots::new(streak.count, &self.data));
                self.phase = Phase::FadeIn(BLACK);
            }
            Phase::RoomFade(steps, t) => {
                self.phase = if t > 0 { Phase::RoomFade(steps, t - 1) } else { Phase::RoomFade(steps + 1, 1) };
            }
            Phase::FadeIn(y) => self.phase = if y > 1 { Phase::FadeIn(y - 1) } else { Phase::Slots },
            Phase::Slots => {
                if let Some(s) = &mut self.slots {
                    return s.step(pressed, rng, frame, streak, lives, &self.data);
                }
            }
        }
        false
    }

    /// 0x08033618 / 0x08033450.
    fn room_step(&mut self, pressed: Buttons) {
        if self.room == Room::Leaving {
            // 0x03003AE7 set: on to sub-state 0x25 (its first step next frame).
            self.phase = Phase::RoomFade(0, 0);
            return;
        }
        if matches!(self.room, Room::Text | Room::Idle) && pressed.contains(Buttons::A) {
            self.room = Room::Leaving;
            return;
        }
        match self.room {
            Room::Box(n) => {
                self.room = if n + 1 >= BOX_ROWS { Room::Text } else { Room::Box(n + 1) };
            }
            Room::Text => {
                if self.text_timer > 1 {
                    self.text_timer -= 1;
                    return;
                }
                self.text_timer = TEXT_STEP;
                let b = self.message.get(self.printed).copied().unwrap_or(END);
                self.printed += 1;
                if b == END {
                    self.room = Room::Idle;
                }
            }
            Room::Idle | Room::Leaving => {}
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn data() -> SpadeAsset {
        let win = |prize| SlotWin {
            prize,
            sign_at: [0x40, 0, 0, 0],
            sign: [0x21AB, 0x21AE, 0x21AF],
            sparkle_at: [vec![0x80], vec![0x84]],
            end_at: vec![0x88],
            sparkles: [[BLANK; 4], [1; 4], [2; 4], [3; 4]],
        };
        SpadeAsset {
            messages: [vec![0x10, 0x11, NEWLINE, SKIP, 0x12, END], vec![END], vec![END], vec![END]],
            bg2: vec![vec![0; 0x400]; 8],
            wins: [2, 3, 5, 7, 10].into_iter().map(win).collect(),
            cycle: [vec![0x80], vec![0x84], vec![0x80], vec![0x88]],
            ..Default::default()
        }
    }

    fn run(s: &mut Spade, keys: Buttons, frames: u32, frame: &mut u32, streak: &mut Streak, lives: &mut u16) -> bool {
        let rng = Rng::default();
        for k in 0..frames {
            *frame += 1;
            let pressed = if k == 0 { keys } else { Buttons::empty() };
            if s.step(pressed, &rng, *frame, streak, lives) {
                return true;
            }
        }
        false
    }

    /// Toad's room once the way in is over.
    fn in_room(st: Streak) -> Spade {
        let mut s = Spade::new(data(), st);
        let (mut f, mut st, mut l) = (0, st, 4);
        run(&mut s, Buttons::A, (OPEN_BLACK + OPEN_IRIS) as u32, &mut f, &mut st, &mut l);
        assert_eq!((s.phase, s.room), (Phase::Room, Room::Box(0)));
        s
    }

    /// mGBA (World 1's spade): black frames, then the iris: nothing for 3 frames, a
    /// radius of 7 at the fourth (columns 113-126 of line 80), 8 more a frame.
    #[test]
    fn the_way_in_opens_an_iris_on_the_room() {
        let (mut f, mut st, mut l) = (0, Streak::default(), 4);
        let mut s = Spade::new(data(), st);
        let mut radii = Vec::new();
        while let Some(r) = s.iris() {
            radii.push(r);
            run(&mut s, Buttons::empty(), 1, &mut f, &mut st, &mut l);
        }
        assert_eq!(radii.len(), (OPEN_BLACK + OPEN_IRIS) as usize);
        let k = OPEN_BLACK as usize;
        assert!(radii[..k + 3].iter().all(|&r| r < 0));
        assert_eq!((radii[k + 3], radii[k + 4], radii[k + 17]), (7, 15, 119));
        assert_eq!((s.phase, s.box_rows()), (Phase::Room, 0), "the box starts after it");
    }

    #[test]
    fn the_box_takes_eight_frames_then_a_byte_every_four() {
        let (mut f, mut st, mut l) = (0, Streak::default(), 4);
        let mut s = in_room(st);
        run(&mut s, Buttons::empty(), 8, &mut f, &mut st, &mut l);
        assert_eq!((s.box_rows(), s.room), (8, Room::Text));
        // 0x0300384D starts at 4: the first byte on the fourth frame.
        run(&mut s, Buttons::empty(), 3, &mut f, &mut st, &mut l);
        assert_eq!(s.printed, 0);
        run(&mut s, Buttons::empty(), 1, &mut f, &mut st, &mut l);
        assert_eq!(s.printed, 1);
        run(&mut s, Buttons::empty(), 4 * 5, &mut f, &mut st, &mut l);
        assert_eq!(s.room, Room::Idle);
        assert_eq!(s.text_cells(), [(0, 0), (1, 0), (1, 1)]);
    }

    #[test]
    fn a_leaves_the_room_through_the_fades() {
        let (mut f, mut st, mut l) = (0, Streak::default(), 4);
        let mut s = in_room(st);
        run(&mut s, Buttons::A, 1, &mut f, &mut st, &mut l);
        assert_eq!(s.room, Room::Box(1), "A does nothing while the box is drawn");
        run(&mut s, Buttons::empty(), 8, &mut f, &mut st, &mut l);
        run(&mut s, Buttons::A, 1, &mut f, &mut st, &mut l);
        assert_eq!(s.room, Room::Leaving);
        let mut shades = Vec::new();
        while s.phase != Phase::Slots {
            run(&mut s, Buttons::empty(), 1, &mut f, &mut st, &mut l);
            shades.push(s.shade());
        }
        // One frame for 0x08033618, 15 of palette steps (every other frame), the load, 16
        // of BLDY.
        assert_eq!(shades.len(), 1 + 15 + 1 + 16);
        assert_eq!(shades[1], Some(Shade::Subtract(4)));
        assert_eq!(shades[15], Some(Shade::Subtract(32)));
        assert_eq!(shades[16], Some(Shade::Darken(BLACK)));
        assert_eq!(shades[17], Some(Shade::Darken(15)));
        assert_eq!(s.shade(), None);
    }

    /// The top and middle rows stopped on these pictures, the bottom one about to.
    fn stopping(s: &mut Slots, pictures: [u32; 3]) {
        for (i, (r, pic)) in s.reels.iter_mut().zip(pictures).enumerate() {
            *r = Reel { state: if i == 2 { 4 } else { 5 }, timer: 0, pos: pic << 15, picture: pic as u8, ..*r };
        }
    }

    fn slots(count: u8) -> Slots {
        let mut s = Slots::new(count, &data());
        s.step(Buttons::empty(), &Rng::default(), 0, &mut Streak::default(), &mut 0, &data());
        s
    }

    #[test]
    fn rows_spin_at_their_speeds() {
        let mut s = slots(0);
        let mut st = Streak::default();
        s.step(Buttons::empty(), &Rng::default(), 1, &mut st, &mut 0, &data());
        assert_eq!(s.reels.map(|r| r.pos), [0x700, 0xFF_F900, 0x7F0]);
        assert_eq!(s.reels[0].scroll(), 0x0F);
    }

    #[test]
    fn a_stopped_row_slows_down_snaps_and_wobbles_onto_a_picture() {
        let mut s = slots(0);
        let mut st = Streak::default();
        let d = data();
        let rng = Rng::default();
        s.step(Buttons::A, &rng, 1, &mut st, &mut 0, &d);
        assert_eq!(s.reels[0].state, 1);
        let mut f = 1;
        while s.reels[0].state != 5 {
            f += 1;
            s.step(Buttons::empty(), &rng, f, &mut st, &mut 0, &d);
            assert!(f < 400);
        }
        let r = s.reels[0];
        assert_eq!(r.pos >> 8 & 0x7F, 0, "on a picture's edge");
        assert_eq!(r.picture, (r.pos >> 15 & 3) as u8);
    }

    #[test]
    fn three_of_a_kind_wins_and_counts_the_lives_in() {
        let mut s = slots(0);
        let d = data();
        stopping(&mut s, [3, 3, 3]);
        let mut st = Streak::default();
        let mut lives = 4;
        s.step(Buttons::empty(), &Rng::default(), 1, &mut st, &mut lives, &d);
        assert_eq!((s.state, s.prize), (4, 5), "star: 5 lives");
        assert_eq!(st, Streak { count: 1, bonus: 3 });
        assert_eq!(s.bg2[0x40..0x43], [0x21AB, 0x21AE, 0x21AF]);
        let mut f = 1;
        let mut risen = None;
        while s.state != 3 {
            f += 1;
            s.step(Buttons::empty(), &Rng::default(), f, &mut st, &mut lives, &d);
            if risen.is_none() && s.prize_y < PRIZE_STOP_Y {
                risen = Some(f);
            }
            assert!(f < 2000);
        }
        assert_eq!(lives, 9);
        assert_eq!(s.prize_y, 0xF0 - 43 * 4);
        // 0x60 frames of waiting, one to state 5, then 43 steps up.
        assert_eq!(risen, Some(1 + 0x60 + 1 + 43));
    }

    #[test]
    fn a_loss_clears_the_streak_and_three_wins_make_a_bonus_round() {
        let d = data();
        let mut s = slots(0);
        stopping(&mut s, [0, 2, 1]);
        let mut st = Streak { count: 2, bonus: 4 };
        s.step(Buttons::empty(), &Rng::default(), 1, &mut st, &mut 0, &d);
        assert_eq!((s.state, s.reels[2].state), (1, 5));
        for f in 2..2 + RESULT_WAIT as u32 {
            s.step(Buttons::empty(), &Rng::default(), f, &mut st, &mut 0, &d);
        }
        assert_eq!((s.state, st), (2, Streak::default()));

        let mut s = slots(2);
        stopping(&mut s, [2, 0, 2]);
        let mut st = Streak { count: 2, bonus: 4 };
        s.step(Buttons::empty(), &Rng::default(), 1, &mut st, &mut 0, &d);
        assert_eq!(s.prize, 2);
        assert_eq!(st, Streak { count: 4, bonus: 5 }, "0x08143E3F[5]");
        // In the bonus round every stop wins.
        let mut s = slots(4);
        stopping(&mut s, [1, 0, 3]);
        let mut st2 = st;
        s.step(Buttons::empty(), &Rng::default(), 1, &mut st2, &mut 0, &d);
        assert_eq!((s.prize, st2), (3, Streak::default()));
    }
}
