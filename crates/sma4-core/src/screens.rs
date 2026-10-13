//! What is on screen besides the level: the world map (shown before each attempt) and the
//! game-over screen, plus the death sequence that leads to them (see docs/re-notes.md,
//! "World map screen", "Game over" and "Death").

/// Death (0x0804A474 -> dying state 0x080A8DAC): the player freezes in the death pose for
/// 0x30 frames, then hops: speed byte 0xC0 (-64), +2 per frame (0x080A8FA4), and the level
/// ends once it has dropped off the bottom of the screen.
pub const DEATH_FREEZE: u8 = 0x30;
pub const DEATH_HOP: i32 = -64;
pub const DEATH_GRAVITY: i32 = 2;
/// Off the screen: the ROM waits for the player's screen y to reach 0xB0.
pub const DEATH_DROP: i32 = 0xB0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dying {
    pub freeze: u8,
    /// Speed (1/16 px per frame) and how far the player has dropped below where it died.
    pub vy: i32,
    pub drop: i32,
}

impl Dying {
    pub fn new() -> Self {
        Self { freeze: DEATH_FREEZE, vy: DEATH_HOP, drop: 0 }
    }

    /// One frame; returns the y movement in 1/256 px, or None when the sequence is over.
    pub fn step(&mut self) -> Option<i32> {
        if self.freeze > 0 {
            self.freeze -= 1;
            return Some(0);
        }
        self.vy = (self.vy + DEATH_GRAVITY).min(0x40);
        let dy = self.vy * 16;
        self.drop += dy;
        (self.drop < DEATH_DROP * 256).then_some(dy)
    }
}

impl Default for Dying {
    fn default() -> Self {
        Self::new()
    }
}

/// Game over (mode 1 sub-states 0x29/0x2A, 0x080138D0 / 0x08013C88): "GAME" slides in from
/// the left and "OVER" from the right while Mario's head drops in and bounces twice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameOver {
    /// Letter offset 0x03003DB4: 0xC0 down to 0x0C, 4 per frame.
    pub slide: u8,
    /// Frames shown once the letters are in place (0x03003DB3).
    pub hold: u8,
    /// Mario head: y (0x03003DB6), speed in 1/256 px (0x03003DB8), bounce phase (0x03003DBA).
    pub head_y: i16,
    head_v: i16,
    phase: u8,
}

/// Letter row y and the head's x (0x08013AB8 / 0x08013B7C).
pub const GAME_OVER_Y: i32 = 0x44;
pub const GAME_OVER_HEAD_X: i32 = 0x72;
const SLIDE_END: u8 = 0x0C;

impl GameOver {
    pub fn new() -> Self {
        Self { slide: 0xC0, hold: 0xFF, head_y: -0x40, head_v: 0, phase: 0 }
    }

    /// One frame; returns false when the screen is over.
    pub fn step(&mut self) -> bool {
        // 0x08013BE8: the head falls; on reaching y 0x44 it bounces at -2, then -1, then rests.
        if self.phase != 3 {
            if self.head_y > 0x43 {
                self.head_v = match self.phase {
                    0 => -0x200,
                    1 => -0x100,
                    _ => self.head_v,
                };
                self.phase += 1;
            }
            if self.phase != 3 {
                self.head_v = (self.head_v + 0x20).min(0x400);
                self.head_y += self.head_v >> 8;
            }
        }
        if self.slide == SLIDE_END {
            self.hold -= 1;
            return self.hold != 0;
        }
        self.slide -= 4;
        true
    }

    /// Screen x of OAM slots 1-10 (cells 0-9 of `game_over.png`): slots 10..6 from
    /// slide + 0x94 leftwards, slots 5..1 from 0x74 - slide leftwards.
    pub fn letter_x(&self) -> [i32; 10] {
        let d = self.slide as i32;
        std::array::from_fn(|i| {
            let slot = i as i32 + 1;
            if slot >= 6 { d + 0x94 - (10 - slot) * 8 } else { 0x74 - d - (5 - slot) * 8 }
        })
    }
}

impl Default for GameOver {
    fn default() -> Self {
        Self::new()
    }
}

/// After game over (mode 1 sub-state 0x2C, menu handlers 0x08135E20): the dimmed map
/// with "GAME OVER / CONTINUE / QUIT". 0x08012F00 waits 0x10 frames; 0x08012FE4 then reads
/// Up/Down (cursor 0x030037CC = 0x58 / 0x68) and A or Start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameOverMenu {
    delay: u8,
    pub quit: bool,
    /// Blink counter 0x03003B74: the cursor shows while bit 4 is clear; Up/Down reset it.
    blink: u8,
}

/// What the menu decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuChoice {
    Continue,
    Quit,
}

/// Cursor position (0x08012E6C): y = cursor - 0x16, x = 0x08135E16[language] (0x4E).
pub const MENU_CURSOR_X: i32 = 0x4E;
const MENU_CONTINUE_Y: i32 = 0x58;
const MENU_QUIT_Y: i32 = 0x68;

impl GameOverMenu {
    pub fn new() -> Self {
        Self { delay: 0x10, quit: false, blink: 0 }
    }

    /// One frame with this frame's newly pressed buttons.
    pub fn step(&mut self, pressed: crate::Buttons) -> Option<MenuChoice> {
        use crate::Buttons;
        if self.delay > 0 {
            self.delay -= 1;
            return None;
        }
        if pressed.contains(Buttons::UP) || pressed.contains(Buttons::DOWN) {
            self.quit = pressed.contains(Buttons::DOWN);
            self.blink = 0;
        } else if pressed.contains(Buttons::A) || pressed.contains(Buttons::START) {
            return Some(if self.quit { MenuChoice::Quit } else { MenuChoice::Continue });
        }
        self.blink = self.blink.wrapping_add(1);
        None
    }

    /// Cursor position on screen, or None on the frames it blinks off.
    pub fn cursor(&self) -> Option<(i32, i32)> {
        let y = if self.quit { MENU_QUIT_Y } else { MENU_CONTINUE_Y } - 0x16;
        (self.blink & 0x10 == 0).then_some((MENU_CURSOR_X, y))
    }
}

impl Default for GameOverMenu {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Screen {
    /// World map with the "WORLD n / MARIO x n" panel; A or Start enters the level.
    Map,
    #[default]
    Level,
    GameOver(GameOver),
    GameOverMenu(GameOverMenu),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn death_freezes_then_hops_off_screen() {
        let mut d = Dying::new();
        let mut frames = 0;
        let mut top = 0;
        let mut y = 0;
        while let Some(dy) = d.step() {
            y += dy;
            top = top.min(y);
            frames += 1;
        }
        assert!(frames > DEATH_FREEZE as u32 + 40);
        assert!(top < -50 * 256, "hops up first");
    }

    #[test]
    fn game_over_menu_waits_then_moves_the_cursor_and_chooses() {
        use crate::Buttons;
        let mut m = GameOverMenu::new();
        for _ in 0..0x10 {
            assert_eq!(m.step(Buttons::A), None, "ignores input at first");
        }
        assert_eq!(m.cursor(), Some((0x4E, 0x42)));
        m.step(Buttons::DOWN);
        assert_eq!(m.cursor(), Some((0x4E, 0x52)));
        for _ in 0..0x10 {
            m.step(Buttons::empty());
        }
        assert_eq!(m.cursor(), None, "blinks");
        assert_eq!(m.step(Buttons::START), Some(MenuChoice::Quit));
        m.step(Buttons::UP);
        assert_eq!(m.step(Buttons::A), Some(MenuChoice::Continue));
    }

    #[test]
    fn game_over_letters_slide_in_and_head_settles() {
        let mut g = GameOver::new();
        let mut frames = 0;
        while g.slide != SLIDE_END {
            g.step();
            frames += 1;
        }
        assert_eq!(frames, (0xC0 - 0x0C) / 4);
        // Final layout from the ROM: "GAME" at x 72..104, "OVER" at 128..160.
        assert_eq!(g.letter_x(), [72, 80, 88, 96, 104, 128, 136, 144, 152, 160]);
        for _ in 0..120 {
            g.step();
        }
        assert_eq!(g.phase, 3);
        assert!((0x40..0x50).contains(&g.head_y));
        let mut more = 0;
        while g.step() {
            more += 1;
        }
        assert!(more > 100);
    }
}
