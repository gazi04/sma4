//! Deterministic game logic. No Bevy, no floats, no wall-clock time: the same input
//! sequence must always produce the same state so it can be diffed against an emulator.

pub mod input;
pub mod player;

pub use input::Buttons;
pub use player::{Collision, Player};

/// GBA frame rate: 16.78 MHz CPU clock / 280_896 cycles per frame.
pub const FRAME_HZ: f64 = 16_777_216.0 / 280_896.0;

pub const SCREEN_W: u32 = 240;
pub const SCREEN_H: u32 = 160;

/// Whole-game state. Grows as subsystems are reverse engineered; layout follows the
/// original RAM structures where that keeps state diffs against the emulator simple.
#[derive(Debug, Default, Clone)]
pub struct Game {
    /// Frames since power-on.
    pub frame: u32,
    pub input: InputState,
    pub collision: Collision,
    pub player: Player,
    /// Where the player respawns after falling out of the level, in pixels.
    pub spawn: (i32, i32),
}

/// Held/pressed tracking, done once per frame like the original's input poll.
#[derive(Debug, Default, Clone, Copy)]
pub struct InputState {
    pub held: Buttons,
    pub pressed: Buttons,
}

impl Game {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn enter_level(&mut self, collision: Collision, spawn: (i32, i32)) {
        self.collision = collision;
        self.spawn = spawn;
        self.player = Player::spawn(spawn.0, spawn.1);
    }

    /// Advances one frame.
    pub fn step(&mut self, held: Buttons) {
        self.input.pressed = held & !self.input.held;
        self.input.held = held;
        self.frame = self.frame.wrapping_add(1);
        if self.collision.width == 0 {
            return;
        }
        self.player.step(held, self.input.pressed, &self.collision);
        // TODO(re): real death/pit handling; for now fall out = respawn.
        if self.player.px().1 > self.collision.pixel_height() + 32 {
            self.player = Player::spawn(self.spawn.0, self.spawn.1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pressed_is_edge_triggered() {
        let mut g = Game::new();
        g.step(Buttons::A);
        assert_eq!(g.input.pressed, Buttons::A);
        g.step(Buttons::A | Buttons::B);
        assert_eq!(g.input.pressed, Buttons::B);
        g.step(Buttons::empty());
        assert!(g.input.pressed.is_empty());
    }
}
