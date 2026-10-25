//! End of a level: the goal roulette card (object type 0x41) and the course-clear sequence
//! it runs (see docs/re-notes.md, "Goal card and course clear").
//!
//! * Init 0x0806826C, main 0x08068248 -> sub-state +0x13 through 0x0816EFBC.
//! * 0 roulette (0x08067914): the card cycles mushroom, flower, star every 8 frames; the
//!   player touching it runs 0x080684D4: clock stopped, enemies turned into coins
//!   (0x0806833C), card added (0x08009FCC), the card flies up (sub-state 4).
//! * 4 (0x08067EF8): the card rises until it is off the screen, then 5 with timer 0x60.
//! * 5 (0x08068004): "COURSE CLEAR" at timer 0x30, the card line at 2, the card in its
//!   frame at 1; the clock is counted into the score (0x08066F08); once the player has
//!   walked off the screen, 7 with timer 0x80.
//! * 7 (0x08068160): when the timer runs out, back to the map (0x03003AE7 + 1).

use crate::level::TILE_PX;
use crate::player::{HITBOX_W, Player, SUB};

/// Enemy-list type of the goal card.
pub const GOAL: u16 = 0x41;

/// Roulette (0x08067914): +0x3C counts up every frame; the card changes when its low 3 bits
/// are 0 (4 bits in a mode flagged by 0x03003D10, not used by normal levels).
const ROULETTE_MASK: u8 = 7;
const CARDS: u8 = 3;
/// Init (0x0806826C): the first card shown is the star if the card is less than 0xB0 px
/// right of the player, the flower otherwise; the counter starts at (frame & 3) + 4.
const NEAR: i32 = 0xB0;
/// Touch box 0x08169F44[+0x64 & 0x3F] with +0x64 = 0x0816A516[0x41] = 0x601: (dx 1, w 13,
/// dy 2, h 8).
const BOX: (i32, i32, i32, i32) = (1, 13, 2, 8);
/// Touch (0x080684D4): speed bytes x 0xFF, y 0xD8 (1/16 px per frame).
const FLY_VX: i32 = -1;
const FLY_VY: i32 = -0x28;
/// The dark box around the roulette fades out after the touch (0x08067F40): +0x3D = 0x10,
/// one step every other frame, blend weights (b, 16 - b).
const BOX_FADE: u8 = 0x10;
/// Sub-state 5 timer and the moments the texts appear.
const RESULTS_TIME: u8 = 0x60;
const SHOW_CLEAR: u8 = 0x30;
const SHOW_CARD_LINE: u8 = 2;
const SHOW_CARD: u8 = 1;
/// Sub-state 7: frames before leaving the level.
const LEAVE_TIME: u8 = 0x80;
/// Clock tally delay 0x03003BB4 = 0xA8 (set at the touch, counted down in sub-state 5);
/// the clock drains once it reaches 0x10.
const TALLY_DELAY: u16 = 0xA8;
const TALLY_START: u16 = 0x10;
/// Points per drained clock step (0x0816EE44, in tens): 10 seconds 50, 1 second 5.
const TALLY_POINTS: [u32; 2] = [50, 5];
/// The player has left once it is 0x120 px into the screen (sub-state 5 compares the low
/// byte of player +0x14 with 0x20; 0x0806743C freezes it at 0x120).
const OFF_SCREEN: i32 = 0x120;
/// Three matching cards: extra lives 0x0816EF0E[card] (mushroom 2, flower 3, star 5).
const BONUS_LIVES: [u16; 3] = [2, 3, 5];

/// Coins the enemies on screen turn into (0x0806833C: effect type 0x11 at the enemy,
/// speed byte -0x40; 0x0808F194 / 0x0808D0E4 move it).
const COIN_VY: i32 = -0x40;
const COIN_GRAVITY_CAP: [i32; 2] = [0x6E, 0x70];
/// It ends once falling at 0x30 (0x0808EFD4): +1 coin, popup type 9 + chain (1000, 2000,
/// ... capped at 1UP), sound 0x12.
const COIN_END_VY: i32 = 0x30;
pub const COIN_POPUP: u16 = 9;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Sub-state 0: waiting for the player, cards cycling.
    Roulette,
    /// Sub-state 4: the card flies up.
    Rising,
    /// Sub-state 5: texts and the clock tally, player walking off.
    Results,
    /// Sub-state 7: a pause, then back to the map.
    Leaving,
}

/// The goal card object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Goal {
    /// Top-left of the object, 1/256 px; the original position (+0x46 / +0x4A) for the box.
    pub x: i32,
    pub y: i32,
    pub home: (i32, i32),
    /// Card shown, 0 mushroom, 1 flower, 2 star (+0x17).
    pub card: u8,
    counter: u8,
    pub phase: Phase,
    /// Sub-state timer +0x23.
    pub timer: u8,
    /// Box fade +0x3D: 0xFF solid, then 0x10 down to 0 (gone).
    pub box_fade: u8,
    tally_delay: u16,
    /// The clock has been counted into the score.
    pub tallied: bool,
}

/// What the goal asks of the game this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Events {
    /// The player just touched the card.
    pub touched: bool,
    /// Points (tens) and seconds taken off the clock by the tally.
    pub points: u32,
    /// The level is over: back to the map.
    pub leave: bool,
}

impl Goal {
    /// Spawned from the enemy list at pixel (x, y); `frame` is the global frame counter.
    pub fn spawn(x: i32, y: i32, player: &Player, frame: u32) -> Self {
        let card = if x - player.px().0 < NEAR { 2 } else { 1 };
        Self {
            x: x * SUB,
            y: y * SUB,
            home: (x, y),
            card,
            counter: (frame & 3) as u8 + 4,
            phase: Phase::Roulette,
            timer: 0,
            box_fade: 0xFF,
            tally_delay: TALLY_DELAY,
            tallied: false,
        }
    }

    pub fn px(&self) -> (i32, i32) {
        (self.x.div_euclid(SUB), self.y.div_euclid(SUB))
    }

    /// The card collected: 1 mushroom, 2 flower, 3 star (0x08009FCC gets roulette + 1).
    pub fn card_id(&self) -> u8 {
        self.card + 1
    }

    /// Which texts show: (course clear, card line, card in the frame).
    pub fn texts(&self) -> (bool, bool, bool) {
        match self.phase {
            Phase::Roulette | Phase::Rising => (false, false, false),
            _ => {
                let t = if self.phase == Phase::Leaving { 0 } else { self.timer };
                (t <= SHOW_CLEAR, t <= SHOW_CARD_LINE, t <= SHOW_CARD)
            }
        }
    }

    fn touches(&self, player: &Player) -> bool {
        let (gx, gy) = self.px();
        let (px, py) = player.px();
        let (dx, w, dy, h) = BOX;
        gx + dx < px + HITBOX_W && px < gx + dx + w && gy + dy < py + player.height() && py < gy + dy + h
    }

    /// One frame. `camera` is the top-left of the screen in level pixels; `clock` the
    /// level clock digits, drained by the tally.
    pub fn step(&mut self, player: &Player, camera: (i32, i32), clock: &mut [u8; 3], frame: u32) -> Events {
        let mut ev = Events::default();
        // Object loop 0x08044ACC: the timer counts down before the main routine.
        self.timer = self.timer.saturating_sub(1);
        if frame & 1 == 0 && self.box_fade != 0xFF && self.box_fade != 0 {
            self.box_fade -= 1;
        }
        match self.phase {
            Phase::Roulette => {
                self.counter = self.counter.wrapping_add(1);
                if self.counter & ROULETTE_MASK == 0 {
                    self.card = (self.card + 1) % CARDS;
                }
                if self.touches(player) {
                    ev.touched = true;
                    self.phase = Phase::Rising;
                    self.box_fade = BOX_FADE;
                    self.tally_delay = TALLY_DELAY;
                }
            }
            Phase::Rising => {
                // 0x08049EA0: x and y move by the speed bytes; no gravity.
                self.x += FLY_VX * 16;
                self.y += FLY_VY * 16;
                // TODO(re): the ROM moves on when +0x1B is set; here, once the card is
                // above the top of the screen.
                if self.px().1 + 16 < camera.1 {
                    self.phase = Phase::Results;
                    self.timer = RESULTS_TIME;
                }
            }
            Phase::Results => {
                let done = self.tally(clock, &mut ev);
                let screen_x = player.px().0 - camera.0;
                if done && self.timer == 0 && screen_x >= OFF_SCREEN {
                    self.phase = Phase::Leaving;
                    self.timer = LEAVE_TIME;
                }
            }
            Phase::Leaving => {
                if self.timer == 0 {
                    ev.leave = true;
                }
            }
        }
        ev
    }

    /// 0x08066F08: count the delay down to 0x10, then drain the clock one step per frame
    /// (tens of seconds while there are any, then seconds) for 50 / 5 tens of points.
    /// Returns true once the clock is empty.
    fn tally(&mut self, clock: &mut [u8; 3], ev: &mut Events) -> bool {
        if self.tallied {
            return true;
        }
        if self.tally_delay != TALLY_START {
            self.tally_delay -= 1;
            return false;
        }
        if clock.iter().all(|&d| d == 0) {
            self.tallied = true;
            return true;
        }
        ev.points += drain(clock, TALLY_POINTS);
        false
    }

    /// Whether the player has walked far enough to stop (0x0806743C sets +0x40).
    pub fn player_stopped(player: &Player, camera: (i32, i32)) -> bool {
        player.px().0 - camera.0 >= OFF_SCREEN
    }
}

/// One step of the clock drain (0x08066F08): ten seconds while there are any, then
/// seconds; `points` (tens) per ten seconds / per second. The clock must not be empty.
fn drain(clock: &mut [u8; 3], points: [u32; 2]) -> u32 {
    let digit = if clock[0] == 0 && clock[1] == 0 { 2 } else { 1 };
    for d in clock[..=digit].iter_mut().rev() {
        if *d > 0 {
            *d -= 1;
            break;
        }
        *d = 9;
    }
    points[digit - 1]
}

/// The clock tally on its own (0x08066F08 called from an object, e.g. the fortress's ball):
/// a delay counted down to 0x10, the drain, then `leave` frames (the object's +0x23 = 0x40,
/// leaving at 1) before the level ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockTally {
    delay: u16,
    points: [u32; 2],
    leave: Option<u8>,
}

const TALLY_LEAVE: u8 = 0x40;

impl ClockTally {
    pub fn new(delay: u16, points: [u32; 2]) -> Self {
        Self { delay, points, leave: None }
    }

    /// One frame: points (tens) earned, and whether the level ends.
    pub fn step(&mut self, clock: &mut [u8; 3]) -> (u32, bool) {
        if let Some(t) = &mut self.leave {
            *t -= 1;
            return (0, *t == 1);
        }
        if self.delay != TALLY_START {
            self.delay -= 1;
            return (0, false);
        }
        if clock.iter().all(|&d| d == 0) {
            self.leave = Some(TALLY_LEAVE);
            return (0, false);
        }
        (drain(clock, self.points), false)
    }
}

/// Extra lives for three matching cards (0x0816EF0E), or 0.
pub fn bonus_lives(cards: &[u8; 3]) -> u16 {
    if cards[0] != 0 && cards.iter().all(|&c| c == cards[0]) { BONUS_LIVES[(cards[0] - 1) as usize] } else { 0 }
}

/// A coin an enemy turned into at the goal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EndCoin {
    /// Top-left, 1/256 px.
    pub x: i32,
    pub y: i32,
    vy: i32,
    /// Spin counter +0x13: frame (counter >> 2) & 3, like the block coin.
    pub counter: u8,
}

impl EndCoin {
    pub fn new(x: i32, y: i32) -> Self {
        Self { x: x * SUB, y: y * SUB, vy: COIN_VY, counter: 0 }
    }

    pub fn px(&self) -> (i32, i32) {
        (self.x.div_euclid(SUB), self.y.div_euclid(SUB))
    }

    /// One frame; false when it is collected (0x0808EFD4).
    pub fn step(&mut self) -> bool {
        self.counter = self.counter.wrapping_add(1);
        // 0x0808D0E4: move, then +2 while below 0x6E; 0x0808F194: +2 more below 0x70.
        self.y += self.vy * 16;
        for cap in COIN_GRAVITY_CAP {
            if self.vy < cap {
                self.vy += 2;
            }
        }
        self.vy < COIN_END_VY
    }

    /// Spin frame in `coin_spin.png`.
    pub fn spin_frame(&self) -> usize {
        ((self.counter >> 2) & 3) as usize
    }
}

/// Where a list entry at (col, row) puts the goal card.
pub fn spawn_px(col: u16, row: u8) -> (i32, i32) {
    (col as i32 * TILE_PX, row as i32 * TILE_PX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn goal_at(x: i32, y: i32, p: &Player) -> Goal {
        Goal::spawn(x, y, p, 0)
    }

    #[test]
    fn roulette_cycles_every_8_frames() {
        let p = Player::spawn(0, 0);
        let mut g = goal_at(400, 64, &p);
        assert_eq!(g.card, 1, "far: starts on the flower");
        let mut clock = [0; 3];
        let mut seen = Vec::new();
        for f in 0..48 {
            g.step(&p, (0, 0), &mut clock, f);
            seen.push(g.card);
        }
        // Counter starts at 4: first change after 4 frames, then every 8.
        assert_eq!(seen[2], 1);
        assert_eq!(seen[3], 2);
        assert_eq!(seen[11], 0);
        assert_eq!(seen[19], 1);
    }

    #[test]
    fn touching_the_card_sends_it_up_and_the_clock_is_tallied() {
        let p0 = Player::spawn(0, 0);
        let mut g = goal_at(100, 200, &p0);
        let touching = Player::spawn(100, 196);
        let mut clock = [1, 2, 3];
        let ev = g.step(&touching, (0, 100), &mut clock, 1);
        assert!(ev.touched);
        assert_eq!(g.phase, Phase::Rising);
        let card = g.card_id();
        let mut f = 2;
        while g.phase == Phase::Rising {
            g.step(&touching, (0, 100), &mut clock, f);
            f += 1;
        }
        assert!(g.px().1 < 100 - 16);
        assert_eq!(g.card_id(), card, "the card stays the one touched");
        // Delay 0xA8 -> 0x10, then 12 tens + 3 seconds.
        let (mut points, mut frames) = (0, 0);
        while !g.tallied {
            points += g.step(&touching, (0, 100), &mut clock, f).points;
            f += 1;
            frames += 1;
        }
        assert_eq!(clock, [0, 0, 0]);
        assert_eq!(points, 12 * 50 + 3 * 5, "50 points a second");
        assert_eq!(frames, (TALLY_DELAY - TALLY_START) as u32 + 12 + 3 + 1);
        assert_eq!(g.texts(), (true, true, true));
    }

    #[test]
    fn leaves_once_the_player_is_off_screen() {
        let p0 = Player::spawn(0, 0);
        let mut g = goal_at(100, 200, &p0);
        g.phase = Phase::Results;
        g.tallied = true;
        let mut clock = [0; 3];
        let on_screen = Player::spawn(150, 196);
        for f in 0..200 {
            g.step(&on_screen, (0, 100), &mut clock, f);
        }
        assert_eq!(g.phase, Phase::Results);
        let gone = Player::spawn(OFF_SCREEN, 196);
        g.step(&gone, (0, 100), &mut clock, 0);
        assert_eq!(g.phase, Phase::Leaving);
        let mut frames = 0;
        while !g.step(&gone, (0, 100), &mut clock, 0).leave {
            frames += 1;
        }
        assert_eq!(frames + 1, LEAVE_TIME as u32);
    }

    #[test]
    fn enemy_coin_pops_up_and_ends_falling() {
        let mut c = EndCoin::new(0, 100);
        let mut frames = 1;
        let mut top = 100;
        while c.step() {
            top = top.min(c.px().1);
            frames += 1;
        }
        // -0x40 + 4 per frame: 28 frames to reach 0x30.
        assert_eq!(frames, 28);
        assert!(top < 100 - 30);
    }

    #[test]
    fn three_matching_cards_give_lives() {
        assert_eq!(bonus_lives(&[3, 3, 3]), 5);
        assert_eq!(bonus_lives(&[1, 1, 1]), 2);
        assert_eq!(bonus_lives(&[1, 2, 1]), 0);
        assert_eq!(bonus_lives(&[0, 0, 0]), 0);
    }
}
