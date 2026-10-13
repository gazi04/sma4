//! Status bar state and drawing: world, lives, clock, coins, score and P-meter, plus the
//! score popups (see docs/re-notes.md, "Status bar and score").
//!
//! * Status bar update 0x08004A60 (BG0 map): P-meter 0x08003BEC, score 0x08004290, clock
//!   0x080044B8 / 0x08004434, coins 0x08003E84, lives 0x08004058; world 0x08004B6C and the
//!   character icon 0x08004ACC.
//! * Score popups 0x08093560 / 0x08093A48 / 0x08093750.

use sma4_assets::{HUD_COLS, HUD_ROWS, HudAsset};

/// Lives at the start of a game (0x08012BA8 stores 4; the bar shows it + 1).
pub const START_LIVES: u16 = 4;
/// Lives after the last one is lost (bit 15 set): the bars show 0 (0x08004058).
pub const NO_LIVES: u16 = 0xFFFF;
/// Clock: one second is 0x28 + 1 frames (0x03003D0E reloads with 0x28 when it wraps).
const CLOCK_RELOAD: u8 = 0x28;
/// Score popup lifetime (+9 = 0x30); its points are added when the timer passes 0x2B.
const POPUP_FRAMES: u8 = 0x30;
const POPUP_SCORE_AT: u8 = 0x2B;
/// Points (in tens) per popup type 1-12 (0x08171DDE); types 13-15 are 1-3 extra lives.
const POPUP_POINTS: [u32; 12] = [1, 2, 4, 8, 10, 20, 40, 80, 100, 200, 400, 800];
pub const POPUP_1UP: u8 = 13;
/// Extra-life popups zoom in (0x080938B4 affine scale): 0x200 (half size) while the timer
/// is above 0x30, then timer * 16 - 0xF0, normal size (0x100) below 0x20.
pub fn life_popup_scale(timer: u8) -> u16 {
    match timer {
        0..0x20 => 0x100,
        0x20..=0x30 => timer as u16 * 16 - 0xF0,
        _ => 0x200,
    }
}
/// Popup rise: one pixel up on frames where 0x08171DFC[timer >> 4] & frame == 0.
const POPUP_RISE_MASK: [u32; 4] = [3, 3, 1, 0];
/// Score is kept in tens; the bar shows 6 digits plus a fixed 0 (max 999999).
const SCORE_MAX: u32 = 999_999;
/// Empty cell in the in-level bar (0x08003DF4 / 0x08004058 use 0xFF there).
const BLANK: u16 = 0xFF;
/// Map positions (u16 index in BG0 row 0): world digit, icon, lives, clock, coins, score.
const WORLD_AT: usize = 4;
const ICON_AT: usize = 6;
const LIVES_AT: usize = 9;
const CLOCK_AT: usize = 14;
const COINS_AT: usize = 19;
const SCORE_AT: usize = 22;
/// P-meter: rows 18-19 from column 6 (map offset 0x48C).
const P_METER_AT: usize = 18 * HUD_COLS + 6;

/// A "100" / "1-UP" number floating up from where points were earned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScorePopup {
    /// Top-left in level pixels.
    pub x: i32,
    pub y: i32,
    /// Popup type 1-12 (`sma4_assets::score_popups_path` cell kind - 1) or 13-15 (extra
    /// lives, `life_popups_path` cell kind - 13).
    pub kind: u8,
    pub timer: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    /// World number (1-8).
    pub world: u8,
    /// Lives as stored (0x03002A6A); the bar shows lives + 1.
    pub lives: u16,
    /// Score in tens (0x03003854).
    pub score: u32,
    /// Coins 0-99 (0x03002C2E + 0x2A).
    pub coins: u8,
    /// Clock digits (0x03003D0B..0D) and the per-second counter (0x03003D0E).
    pub clock: [u8; 3],
    clock_sub: u8,
    /// Whether this level has a clock.
    pub timed: bool,
    pub popups: Vec<ScorePopup>,
    /// Goal cards collected (0x03002C52..54): 0 none, 1 mushroom, 2 flower, 3 star.
    pub cards: [u8; 3],
}

impl Default for Status {
    fn default() -> Self {
        Self {
            world: 1,
            lives: START_LIVES,
            score: 0,
            coins: 0,
            clock: [0; 3],
            clock_sub: 0,
            timed: false,
            popups: Vec::new(),
            cards: [0; 3],
        }
    }
}

impl Status {
    /// Sets the clock to `seconds` (0 = no clock), as the level loader does.
    pub fn start_clock(&mut self, seconds: u16) {
        self.timed = seconds != 0;
        self.clock = [(seconds / 100 % 10) as u8, (seconds / 10 % 10) as u8, (seconds % 10) as u8];
        // A full second before the first tick (0x08014F20 reloads 0x03003D0E with 0x28).
        self.clock_sub = CLOCK_RELOAD;
    }

    pub fn seconds(&self) -> u16 {
        self.clock[0] as u16 * 100 + self.clock[1] as u16 * 10 + self.clock[2] as u16
    }

    /// 0x080044B8: one clock frame. Returns true when time just ran out.
    pub fn tick_clock(&mut self) -> bool {
        if !self.timed || self.seconds() == 0 {
            return false;
        }
        self.clock_sub = self.clock_sub.wrapping_sub(1);
        if self.clock_sub != 0xFF {
            return false;
        }
        self.clock_sub = CLOCK_RELOAD;
        for d in self.clock.iter_mut().rev() {
            if *d > 0 {
                *d -= 1;
                break;
            }
            *d = 9;
        }
        self.seconds() == 0
    }

    pub fn add_score(&mut self, tens: u32) {
        self.score = (self.score + tens).min(SCORE_MAX);
    }

    /// Coins (0x08003E84): every 100th coin gives a life.
    pub fn add_coin(&mut self) {
        self.coins += 1;
        if self.coins > 99 {
            self.coins -= 100;
            self.lives += 1;
        }
    }

    /// 0x08009FCC: the card goes in the first empty slot; with all three full, the row
    /// moves left to make room.
    pub fn add_card(&mut self, card: u8) {
        if let Some(slot) = self.cards.iter_mut().find(|c| **c == 0) {
            *slot = card;
        } else {
            self.cards.rotate_left(1);
            self.cards[2] = card;
        }
    }

    /// 0x08093560: a popup of type `kind` at level pixel (x, y) (the object's top-left).
    pub fn popup(&mut self, x: i32, y: i32, kind: u8) {
        let kind = kind.min(POPUP_1UP + 2);
        // 0x08171D9C[class + 1] = 4 for these objects; y + 4. The wider extra-life popups
        // sit 8 px further left and 4 px higher.
        let (dx, dy) = if kind >= POPUP_1UP { (-8, -4) } else { (0, 0) };
        self.popups.push(ScorePopup { x: x + 4 + dx, y: y + 4 + dy, kind, timer: POPUP_FRAMES });
    }

    /// 0x08093654: popup for the `chain`-th enemy in a row (100, 200, ... 8000, then 1-UPs).
    pub fn chain_popup(&mut self, x: i32, y: i32, chain: u16) {
        self.popup(x, y, (chain + 5).min(POPUP_1UP as u16) as u8);
    }

    /// 0x08093A48 / 0x08093B5C: popups count down, give their points or lives, and drift
    /// up.
    pub fn step_popups(&mut self, frame: u32) {
        let mut earned = 0;
        let mut lives = 0;
        self.popups.retain_mut(|p| {
            if p.timer == 0 {
                return false;
            }
            let was = p.timer;
            p.timer -= 1;
            if was == POPUP_SCORE_AT {
                if p.kind < POPUP_1UP {
                    earned += POPUP_POINTS[(p.kind - 1) as usize];
                } else {
                    lives += (p.kind - POPUP_1UP + 1) as u16;
                }
            }
            if POPUP_RISE_MASK[(p.timer >> 4) as usize] & frame == 0 {
                p.y -= 1;
            }
            true
        });
        self.add_score(earned);
        self.lives += lives;
    }

    /// The status-bar map (`HUD_COLS` x `HUD_ROWS` entries) for this frame: the ROM's
    /// template with the update routines' writes applied.
    pub fn map(&self, hud: &HudAsset, p_meter: ([bool; 6], bool)) -> Vec<u16> {
        let mut m = hud.template.clone();
        let digit = |d: u32| hud.digits[d as usize % 10];
        // White digits: the routines subtract 0x1000 (palette 15).
        let white = |d: u32| digit(d).wrapping_sub(0x1000);

        m[WORLD_AT] = digit(self.world as u32);
        m[ICON_AT] = hud.icon[0];
        m[ICON_AT + 1] = hud.icon[1];

        // Lives (0x08004058): lives + 1, up to three digits, no leading zeros.
        for (i, c) in self.lives_cells(&hud.digits, BLANK).into_iter().enumerate() {
            m[LIVES_AT + i] = c;
        }

        // Clock (0x08004434): yellow digits.
        for (i, &d) in self.clock.iter().enumerate() {
            m[CLOCK_AT + i] = digit(d as u32);
        }

        // Coins (0x08003DF4): two digits, blank tens.
        let c = self.coins as u32;
        m[COINS_AT] = if c >= 10 { white(c / 10) } else { BLANK };
        m[COINS_AT + 1] = white(c % 10);

        // Score (0x08004290): six digits with leading zeros; the template's 0 follows.
        let mut s = self.score;
        for (i, place) in [100_000, 10_000, 1_000, 100, 10, 1].into_iter().enumerate() {
            m[SCORE_AT + i] = white(s / place);
            s %= place;
        }

        // P-meter (0x08003BEC): arrows 0x2F0/0x2F1, "P" 0x2F2+3 or lit 0x2F4+5; the second
        // row is tile + 6.
        let (arrows, lit) = p_meter;
        let mut cells = [0u16; 8];
        for (i, &on) in arrows.iter().enumerate() {
            cells[i] = if on { 0x2F1 } else { 0x2F0 };
        }
        cells[6] = if lit { 0x2F4 } else { 0x2F2 };
        cells[7] = cells[6] + 1;
        for (i, &t) in cells.iter().enumerate() {
            m[P_METER_AT + i] = t;
            m[P_METER_AT + HUD_COLS + i] = t + 6;
        }
        debug_assert_eq!(m.len(), HUD_COLS * HUD_ROWS);
        m
    }
}

/// Map-screen status bar (the same routines in map mode, row 3 of BG0): world +0xCA,
/// icon +0xCC, lives +0xD2, coins +0xDA, score +0xE0; empty cells are 0x2AD there.
const MAP_ROW: usize = 3 * HUD_COLS;
const MAP_BLANK: u16 = 0x2AD;
/// Panel (BG3, 0x0800B0E4's VBlank list): world digit at row 10 col 18 (tile 0x3F0 + n),
/// lives in row 13 cols 19-21 (the bar's lives cells + 0x160; empty 0x3FE).
const PANEL_WORLD_AT: usize = 10 * HUD_COLS + 18;
const PANEL_LIVES_AT: usize = 13 * HUD_COLS + 19;
const PANEL_DIGIT: u16 = 0x3F0;
const PANEL_BLANK: u16 = 0x3FE;

impl Status {
    /// Lives cells as 0x08004058 writes them: (hundreds,) tens or `blank`, ones, `blank`.
    fn lives_cells(&self, digits: &[u16; 10], blank: u16) -> Vec<u16> {
        let white = |d: u32| digits[d as usize % 10].wrapping_sub(0x1000);
        if self.lives == NO_LIVES {
            return vec![blank, white(0), blank];
        }
        let shown = self.lives.min(0x3E6) as u32 + 1;
        let mut cells = Vec::new();
        if shown >= 100 {
            cells.push(white(shown / 100));
        }
        let tens = shown % 100 / 10;
        cells.push(if tens != 0 || shown >= 100 { white(tens) } else { blank });
        cells.push(white(shown % 10));
        if self.lives < 99 {
            cells.push(blank);
        }
        cells
    }

    /// The map screen's status bar: `template` (BG0, 32x32) with this state written in.
    pub fn map_bar(&self, template: &[u16], hud: &HudAsset) -> Vec<u16> {
        let mut m = template.to_vec();
        let white = |d: u32| hud.digits[d as usize % 10].wrapping_sub(0x1000);
        m[MAP_ROW + 5] = hud.digits[self.world as usize % 10];
        m[MAP_ROW + 6] = hud.icon[0];
        m[MAP_ROW + 7] = hud.icon[1];
        for (i, c) in self.lives_cells(&hud.digits, MAP_BLANK).into_iter().enumerate() {
            m[MAP_ROW + 9 + i] = c;
        }
        let c = self.coins as u32;
        m[MAP_ROW + 13] = if c >= 10 { white(c / 10) } else { MAP_BLANK };
        m[MAP_ROW + 14] = white(c % 10);
        let mut s = self.score;
        for (i, place) in [100_000, 10_000, 1_000, 100, 10, 1].into_iter().enumerate() {
            m[MAP_ROW + 16 + i] = white(s / place);
            s %= place;
        }
        m
    }

    /// The "WORLD n / MARIO x n" panel: `template` (BG3, 32x32) with world and lives.
    pub fn map_panel(&self, template: &[u16], hud: &HudAsset) -> Vec<u16> {
        let mut m = template.to_vec();
        m[PANEL_WORLD_AT] = PANEL_DIGIT + self.world as u16 % 10;
        let cells = self.lives_cells(&hud.digits, MAP_BLANK);
        for (i, &c) in cells.iter().take(3).enumerate() {
            m[PANEL_LIVES_AT + i] = if c == MAP_BLANK { PANEL_BLANK } else { (c + 0x160) & 0x3FF };
        }
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hud() -> HudAsset {
        HudAsset {
            template: vec![0xFF; HUD_COLS * HUD_ROWS],
            digits: std::array::from_fn(|i| 0x290 + i as u16),
            icon: [0xF288, 0xF289],
        }
    }

    #[test]
    fn map_screen_shows_world_and_lives() {
        let mut s = Status::default();
        s.lives = 11;
        let bar = s.map_bar(&vec![0; 1024], &hud());
        assert_eq!(bar[MAP_ROW + 5], 0x291);
        assert_eq!(&bar[MAP_ROW + 9..MAP_ROW + 12], &[0xF291, 0xF292, MAP_BLANK], "lives 11 shows 12");
        let panel = s.map_panel(&vec![0; 1024], &hud());
        assert_eq!(panel[PANEL_WORLD_AT], 0x3F1);
        assert_eq!(&panel[PANEL_LIVES_AT..PANEL_LIVES_AT + 3], &[0x3F1, 0x3F2, PANEL_BLANK]);
    }

    #[test]
    fn clock_ticks_once_every_41_frames_and_runs_out() {
        let mut s = Status::default();
        s.start_clock(300);
        let mut frames = 0;
        while s.seconds() == 300 {
            s.tick_clock();
            frames += 1;
        }
        assert_eq!((frames, s.clock), (0x29, [2, 9, 9]));
        for _ in 0..41 {
            s.tick_clock();
        }
        assert_eq!(s.seconds(), 298);
        s.start_clock(1);
        let out = (0..100).any(|_| s.tick_clock());
        assert!(out && s.seconds() == 0);
    }

    #[test]
    fn hundred_coins_give_a_life() {
        let mut s = Status::default();
        for _ in 0..100 {
            s.add_coin();
        }
        assert_eq!((s.coins, s.lives), (0, START_LIVES + 1));
    }

    #[test]
    fn popups_add_their_points_after_five_frames() {
        let mut s = Status::default();
        s.chain_popup(0, 100, 0); // 100 points
        s.chain_popup(0, 100, 3); // 800 points
        for f in 0..5 {
            s.step_popups(f);
        }
        assert_eq!(s.score, 0);
        s.step_popups(5);
        assert_eq!(s.score, 10 + 80);
        for f in 6..60 {
            s.step_popups(f);
        }
        assert!(s.popups.is_empty());
        s.chain_popup(0, 100, 20);
        assert_eq!(s.popups[0].kind, POPUP_1UP, "a long chain ends in 1-UPs");
        for f in 0..10 {
            s.step_popups(f);
        }
        assert_eq!(s.lives, START_LIVES + 1);
        assert_eq!(life_popup_scale(0x31), 0x200);
        assert_eq!(life_popup_scale(0x10), 0x100);
    }

    #[test]
    fn bar_shows_world_lives_clock_coins_and_score() {
        let mut s = Status::default();
        s.start_clock(300);
        s.coins = 7;
        s.score = 1234;
        let m = s.map(&hud(), ([true, true, false, false, false, false], false));
        assert_eq!(m[WORLD_AT], 0x291);
        assert_eq!(&m[LIVES_AT..LIVES_AT + 3], &[BLANK, 0xF295, BLANK], "lives 4 shows 5");
        assert_eq!(&m[CLOCK_AT..CLOCK_AT + 3], &[0x293, 0x290, 0x290]);
        assert_eq!(&m[COINS_AT..COINS_AT + 2], &[BLANK, 0xF297]);
        assert_eq!(&m[SCORE_AT..SCORE_AT + 6], &[0xF290, 0xF290, 0xF291, 0xF292, 0xF293, 0xF294]);
        assert_eq!(&m[P_METER_AT..P_METER_AT + 3], &[0x2F1, 0x2F1, 0x2F0]);
        assert_eq!(m[P_METER_AT + HUD_COLS], 0x2F7);
    }
}
