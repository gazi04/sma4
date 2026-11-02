//! The Advance Coin (enemy-list type 0xCD, init 0x08076C94, main 0x08077720; phase handlers
//! 0x0816FA1C) and the coin quota entry that goes with it (type 0x1A8, 0x0804B768). See
//! docs/re-notes.md, "Advance Coin" (checked in mGBA against the real game).
//!
//! * The ROM only spawns it once the game has been beaten (0x0804B768: `0x03002C85` is not
//!   0); `Game::progress` stands for that byte. Once one has been hit (`0x03007848 +
//!   0xE67`, kept through area switches) the init 0x08076C94 removes any other.
//! * Type 0x1A8's row byte is the level's quota: `row & 0x7F` coins (`0x0300379F`), and
//!   `(row & 1) + 1` the kind of map reward (`0x0300379E`). The level's coin counter
//!   (`0x030037A0`) counts the coins Mario collects in the whole level, across its areas.
//! * Phase 0: drawn as a 16x16 OBJ while its screen x is in [-0x20, 0x100) (0x08076CD8);
//!   an invisible solid tile (0x79) sits on its cell while it is well inside the screen
//!   (0x080750F0). The player touches it like a block (0x0807536C, `contact`): pushed
//!   aside, landing on it, or, coming from below, his head hits it: phase 1.
//! * Phase 1 (0x08077160, the next frame): the coin becomes a screen sprite with a y speed
//!   of -0x30 and an x speed that takes it to x = 0x10 while it rises and falls to y =
//!   0x90 (phase 2, 0x08077218); the whole game waits (`+0xDF8` -> `+0xBD1`).
//! * Phase 3 (0x080772A4): a 0x20-frame sparkle; at timer 0x0F the quota is compared:
//!   short of it, sound 0x5C and sub-state `+0x3B` = 1 (the corner then shows the coins
//!   to go, phase 4 0x080773E4); otherwise, or once the quota is reached, `+0x3B` = 2, a
//!   second sparkle and phase 5 (0x0807742C): the collected coin sits in the corner. If
//!   the player dies then (`+0x62`) it jumps out of the corner and falls away.
//! * The timer `+0x23` counts down once a frame: by the handler while the game waits, by
//!   the object loop otherwise (before the handler).
//!
//! Positions in 1/256 px, speed bytes in 1/16 px per frame.

use sma4_assets::EnemySpawn;
use crate::level::TILE_PX;
use crate::player::{Player, SUB};

/// Enemy-list type of the coin.
pub const COIN: u16 = 0xCD;
/// Enemy-list type of the quota entry (the row byte carries the numbers).
pub const QUOTA: u16 = 0x1A8;
/// The screen position the coin flies to (x, y); in phase 5 it sits at x = 0x10.
const HOME: (i32, i32) = (0x10, 0x90);
/// Launch speed (+0x12 = 0xD0), gravity and its cap (0x08077218).
const LAUNCH: i32 = -0x30;
const GRAVITY: i32 = 2;
const MAX_FALL: i32 = 0x30;
/// Sparkle time (+0x23).
const BANNER: u8 = 0x1F;
/// The timer value at which the quota is checked.
const CHECK_AT: u8 = 0x0F;
/// Solid tile window (0x080750F0): screen x in [0x10, 0xE0), y in [0x10, 0x90).
const SOLID_X: (i32, i32) = (0x10, 0xE0);
const SOLID_Y: (i32, i32) = (0x10, 0x90);
/// Draw window (0x08076CD8): screen x in [-0x20, 0x100).
const DRAW_X: (i32, i32) = (-0x20, 0x100);
/// The coin's touch box (0x08075330): x + 1, 14 wide; y - 3, 19 high.
const BOX: (i32, i32, i32, i32) = (1, 0x0E, -3, 0x13);
/// The player's touch boxes from the 16x32 frame's top-left (0x080485A4, 0x08169624):
/// small or crouching (index 0) and big (index 1).
const PLAYER_BOX_SMALL: (i32, i32, i32, i32) = (4, 8, 17, 13);
const PLAYER_BOX_BIG: (i32, i32, i32, i32) = (3, 10, 5, 25);
/// 0x0807536C: an overlap less than this deep (19 - 3, or the player's box height - 3)
/// pushes the player aside instead.
const SIDE_SLACK: i32 = 3;
/// Phase 5 when the player dies (0x0807742C): jump speed 0xD8, + 3 a frame up to 0x40,
/// y at most 0xF0.
const LEAVE_JUMP: i32 = -0x28;
const LEAVE_GRAVITY: i32 = 3;
const LEAVE_MAX: i32 = 0x40;
const LEAVE_BOTTOM: i32 = 0xF0;
/// The player's dying y speed when the death starts (+0x1D = 0xC0).
const DEATH_HOP: i32 = -0x40;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// In the level (phase 0, and phase 1: hit, taking off next frame).
    Idle,
    /// Flying to the corner, a screen sprite (phase 2).
    Flying,
    /// The sparkle (phase 3).
    Banner,
    /// In the corner, counting (phase 4).
    Counting,
    /// The quota is reached: the collected coin in the corner (phase 5).
    Collected,
}

/// How the player touched the idle coin (0x0807536C).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Contact {
    /// Too shallow an overlap: pushed 1 px aside, x speed 0.
    Side,
    /// Landed on it (or stays on it).
    Top,
    /// His head hit it from below.
    Head,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdvanceCoin {
    /// +0 / +8: the level position until it is hit, then the screen position.
    x: i32,
    y: i32,
    /// +0x10 / +0x12, speed bytes.
    vx: i32,
    vy: i32,
    /// +0x3A (0-5), +0x3B, +0x23, +0x3E (phase 5 has jumped).
    phase: u8,
    sub: u8,
    timer: u8,
    jumped: bool,
    /// +0xDF8: the whole game waits.
    freeze: bool,
}

/// What the coin did this frame.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Step {
    /// A sound to play (0x5B hit, 0x5C not enough coins yet).
    pub sound: Option<u8>,
    /// The player touched it this frame.
    pub contact: Option<Contact>,
}

/// The game state the coin reads.
#[derive(Debug, Clone, Copy)]
pub struct Context {
    /// The camera (0x03003750, 0x03003754).
    pub camera: (i32, i32),
    /// 0x030037A0 and 0x0300379F.
    pub coins: u8,
    pub quota: u8,
    /// The objects are frozen this frame (+0xBD1, from last frame's +0xDF8 and the
    /// player's state): the object loop does not count the timer down.
    pub frozen: bool,
    /// The player is dying (+0x62 != 0): his y speed byte.
    pub dying: Option<i32>,
}

impl AdvanceCoin {
    /// 0x0804B768: the object's x is the column, y the row byte times 16.
    pub fn spawn(s: &EnemySpawn) -> Self {
        Self {
            x: s.col as i32 * TILE_PX * SUB,
            y: s.row as i32 * TILE_PX * SUB,
            vx: 0,
            vy: 0,
            phase: 0,
            sub: 0,
            timer: 0,
            jumped: false,
            freeze: false,
        }
    }

    pub fn phase(&self) -> Phase {
        match self.phase {
            0 | 1 => Phase::Idle,
            2 => Phase::Flying,
            3 => Phase::Banner,
            4 => Phase::Counting,
            _ => Phase::Collected,
        }
    }

    /// The whole game waits (+0xDF8 -> +0xBD1).
    pub fn freezes(&self) -> bool {
        self.freeze
    }

    /// It has been hit (the ROM's +0xE67).
    pub fn taken(&self) -> bool {
        self.phase != 0
    }

    /// The coin's level cell.
    pub fn cell(&self) -> (i32, i32) {
        ((self.x >> 8).div_euclid(TILE_PX), (self.y >> 8).div_euclid(TILE_PX))
    }

    /// Position on the level in px while idle, on the screen once it has been hit.
    pub fn px(&self) -> (i32, i32) {
        (self.x >> 8, self.y >> 8)
    }

    /// +0x3B: 0 until the quota was first compared, 1 coins to go, 2 reached.
    pub fn sub(&self) -> u8 {
        self.sub
    }

    /// +0x23, the sparkle's timer.
    pub fn timer(&self) -> u8 {
        self.timer
    }

    fn screen(&self, camera: (i32, i32)) -> (i32, i32) {
        let (x, y) = self.px();
        (x - camera.0, y - camera.1)
    }

    /// The idle coin's cell is solid (0x080750F0: tile 0x79 while well inside the screen).
    pub fn solid(&self, camera: (i32, i32)) -> bool {
        let (sx, sy) = self.screen(camera);
        self.phase == 0 && (SOLID_X.0..SOLID_X.1).contains(&sx) && (SOLID_Y.0..SOLID_Y.1).contains(&sy)
    }

    /// The coin in the level is drawn (0x08076CD8: screen x in [-0x20, 0x100)).
    pub fn drawn(&self, camera: (i32, i32)) -> bool {
        self.phase <= 1 && (DRAW_X.0..DRAW_X.1).contains(&self.screen(camera).0)
    }

    /// 0x08077720: the object loop's timer, then the phase handler.
    pub fn step(&mut self, c: &Context, player: &mut Player) -> Step {
        let mut out = Step::default();
        if !c.frozen && self.timer != 0 {
            self.timer -= 1;
        }
        match self.phase {
            0 => {
                // 0x080770CC: drawn, not frozen, touched and the head hit it.
                if self.drawn(c.camera) && !c.frozen && c.dying.is_none() {
                    out.contact = self.contact(player, c.camera);
                    if out.contact == Some(Contact::Head) {
                        self.phase = 1;
                        out.sound = Some(0x5B);
                    }
                }
            }
            1 => {
                self.launch(c);
                out.sound = Some(0x5B);
            }
            2 => self.fly(),
            3 => out.sound = self.banner(c),
            4 => {
                // 0x080773E4: the quota was reached while it was counting.
                if c.quota <= c.coins {
                    self.sub = 2;
                }
                if self.sub == 2 {
                    self.phase = 3;
                    self.timer = BANNER;
                }
            }
            _ => self.leave(c),
        }
        out
    }

    /// 0x0807536C (0x08075330 / 0x080485A4 boxes, 0x08048718 overlap) for the idle coin.
    fn contact(&self, p: &mut Player, camera: (i32, i32)) -> Option<Contact> {
        let (sx, sy) = self.screen(camera);
        let (cx, cw, cy, ch) = (sx + BOX.0, BOX.1, sy + BOX.2, BOX.3);
        let (fx, fy) = p.frame();
        let (bdx, pw, bdy, ph) = if p.small_box() { PLAYER_BOX_SMALL } else { PLAYER_BOX_BIG };
        let (px, py) = (fx - camera.0 + bdx, fy - camera.1 + bdy);
        // Each axis: the distance from the box further left (up) to the other, at most its
        // own size.
        let left = px < cx;
        if (px - cx).abs() > if left { pw } else { cw } {
            return None;
        }
        let above = py < cy;
        let dy = (py - cy).abs();
        if dy > if above { ph } else { ch } {
            return None;
        }
        let deep = if above { ph } else { ch };
        if dy - deep < -SIDE_SLACK {
            // Side: 1 px away from the coin, x speed 0.
            p.vx = 0;
            p.x += if left { -SUB } else { SUB };
            return Some(Contact::Side);
        }
        if above {
            // On top: the frame 0x20 above the coin, landed (0x080A3C54).
            if p.vy >= 0 {
                let frame_y = (self.y >> 8) - 0x20;
                p.y += (frame_y - fy) * SUB;
                p.vy = 0;
                p.on_ground = true;
            }
            return Some(Contact::Top);
        }
        // From below: rising, the player stops and moves down 2 px.
        if p.vy < 0 {
            p.vy = 0;
            p.y += 2 * SUB;
        }
        Some(Contact::Head)
    }

    /// 0x08077160, the frame after the hit: to screen coordinates, the speeds, frozen.
    fn launch(&mut self, c: &Context) {
        let (sx, sy) = self.screen(c.camera);
        self.x = sx << 8;
        self.y = sy << 8;
        // 0x080EB530 (0x90 - y) & 0xFFFF / 3, then 0x080EB384 (-(x - 0x10) * 16) / (t + 0x30).
        let t = ((HOME.1 - sy) & 0xFFFF) / 3;
        self.vx = (-((sx - HOME.0) * 16) / (t + 0x30)) as i8 as i32;
        self.vy = LAUNCH;
        self.phase = 2;
        self.freeze = true;
    }

    /// 0x08077218.
    fn fly(&mut self) {
        self.freeze = true;
        self.x += self.vx * 16;
        self.vy = (self.vy + GRAVITY).min(MAX_FALL);
        self.y += self.vy * 16;
        if self.y >> 8 >= HOME.1 {
            self.y = HOME.1 << 8;
            self.phase = 3;
            self.timer = BANNER;
        }
    }

    /// 0x080772A4: the sound to play, if any.
    fn banner(&mut self, c: &Context) -> Option<u8> {
        if self.timer == 0 {
            self.phase += self.sub;
            self.freeze = false;
            return None;
        }
        if self.sub == 0 {
            self.freeze = true;
        }
        if self.freeze {
            self.timer -= 1;
        }
        if self.timer != CHECK_AT {
            return None;
        }
        if self.sub != 0 {
            if self.sub == 2 {
                self.phase = 5;
            }
            return None;
        }
        self.sub = 1;
        if c.coins < c.quota {
            return Some(0x5C);
        }
        self.phase += 1;
        self.freeze = false;
        None
    }

    /// 0x0807742C for type 0xCD: the collected coin in the corner; when the player dies it
    /// jumps out and falls (once his death hop has started).
    fn leave(&mut self, c: &Context) {
        if let Some(vy) = c.dying {
            if !self.jumped {
                if vy != DEATH_HOP {
                    self.vy = LEAVE_JUMP;
                    self.jumped = true;
                }
            } else {
                self.y += self.vy * 16;
                if self.vy < LEAVE_MAX {
                    self.vy += LEAVE_GRAVITY;
                }
                self.y = self.y.min(LEAVE_BOTTOM << 8);
            }
        }
        self.x = HOME.0 << 8;
    }

    /// 0x080775CC: the coins still to collect, as hundreds / tens / units.
    pub fn remaining(&self, coins: u8, quota: u8) -> [u8; 3] {
        let n = quota.saturating_sub(coins);
        [n / 100, n / 10 % 10, n % 10]
    }
}

/// The level's coin quota entry (0x0804B768, type 0x1A8): (quota, kind). The row byte's
/// low bit picks the kind of map reward (1 or 2), its low 7 bits are the quota.
pub fn quota(s: &EnemySpawn) -> (u8, u8) {
    (s.row & 0x7F, (s.row & 1) + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player::Form;

    /// World 1-4's coin at (7, 18), the camera where mGBA had it.
    fn coin() -> AdvanceCoin {
        AdvanceCoin::spawn(&EnemySpawn { kind: COIN, col: 7, row: 18, ..Default::default() })
    }

    fn ctx(coins: u8, quota: u8, frozen: bool) -> Context {
        Context { camera: (36, 267), coins, quota, frozen, dying: None }
    }

    /// A player whose frame's top-left is at (fx, fy), rising.
    fn player_at(fx: i32, fy: i32, form: Form) -> Player {
        let mut p = Player::spawn(0, 0);
        p.set_form(form);
        let (x0, y0) = p.frame();
        p.x += (fx - x0) * SUB;
        p.y += (fy - y0) * SUB;
        p.vy = -0x30 * crate::player::UNIT;
        p
    }

    #[test]
    fn world_1_4_quota_is_44_coins() {
        let (q, kind) = quota(&EnemySpawn { kind: QUOTA, col: 1, row: 44, ..Default::default() });
        assert_eq!((q, kind), (44, 1));
    }

    /// mGBA, World 1-4: the head hit sets it off from frame x 101-123 (small, frame y 285)
    /// and 100-124 (big, frame y 297); the box formula gives exactly these.
    #[test]
    fn head_hit_range_matches_the_rom() {
        for (form, fy, range) in [(Form::Small, 285, 101..=123), (Form::Big, 297, 100..=124)] {
            for fx in 90..135 {
                let c = coin();
                let mut p = player_at(fx, fy, form);
                let got = c.contact(&mut p, (36, 267));
                assert_eq!(got == Some(Contact::Head), range.contains(&fx), "{form:?} at {fx}: {got:?}");
            }
        }
    }

    #[test]
    fn landing_on_it_and_side_pushes() {
        let c = coin();
        // Landing needs a shallow overlap: the box's bottom within 3 px of the coin box's top.
        let mut p = player_at(112, 288 - 0x20 + 1, Form::Big);
        p.vy = 0x10;
        assert_eq!(c.contact(&mut p, (36, 267)), Some(Contact::Top));
        assert_eq!(p.frame().1, 288 - 0x20);
        let mut p = player_at(112 - 10, 288 - 8, Form::Big);
        let x = p.x;
        assert_eq!(c.contact(&mut p, (36, 267)), Some(Contact::Side));
        assert_eq!(p.x, x - SUB);
    }

    /// Recorded in mGBA (World 1-4, hit at camera (36, 267)): phase 1 for a frame, the
    /// flight from screen (76, 21) with speeds (-10, -48) for 88 frames, the sparkle 32
    /// frames (sub-state 1 from timer 0x0F), then counting; with the quota met, the second
    /// sparkle runs unfrozen and turns into phase 5 at timer 0x0F.
    #[test]
    fn sequence_matches_the_rom() {
        let mut c = coin();
        let mut p = player_at(112, 297, Form::Big);
        let mut frozen = false;
        let run = |c: &mut AdvanceCoin, cx: Context, p: &mut Player, frozen: &mut bool| {
            let s = c.step(&Context { frozen: *frozen, ..cx }, p);
            *frozen = c.freezes();
            s
        };
        assert_eq!(run(&mut c, ctx(0, 44, false), &mut p, &mut frozen).contact, Some(Contact::Head));
        assert_eq!(c.phase, 1);
        run(&mut c, ctx(0, 44, false), &mut p, &mut frozen);
        assert_eq!((c.x, c.y, c.vx, c.vy, c.phase), (76 << 8, 21 << 8, -10, -48, 2));
        run(&mut c, ctx(0, 44, false), &mut p, &mut frozen);
        assert_eq!((c.x, c.y, c.vy), (19296, 4640, -46), "the second row mGBA recorded");
        let mut flight = 1;
        while c.phase == 2 {
            run(&mut c, ctx(0, 44, false), &mut p, &mut frozen);
            flight += 1;
        }
        assert_eq!(flight, 88);
        assert_eq!((c.timer, c.sub), (BANNER, 0));
        let mut seq = Vec::new();
        while c.phase == 3 {
            seq.push((c.timer, c.sub));
            run(&mut c, ctx(0, 44, false), &mut p, &mut frozen);
        }
        assert_eq!(seq.len(), 32);
        assert_eq!(seq[16], (15, 1));
        assert_eq!(c.phase(), Phase::Counting);
        assert!(!c.freezes());
        // Quota met (coins poked to 44 in mGBA).
        run(&mut c, ctx(44, 44, frozen), &mut p, &mut frozen);
        assert_eq!((c.phase, c.sub, c.timer), (3, 2, BANNER));
        let mut n = 0;
        while c.phase == 3 {
            run(&mut c, ctx(44, 44, frozen), &mut p, &mut frozen);
            n += 1;
        }
        assert_eq!((c.phase(), c.timer, n), (Phase::Collected, CHECK_AT, 16));
        assert!(!c.freezes());
        for _ in 0..20 {
            run(&mut c, ctx(44, 44, frozen), &mut p, &mut frozen);
        }
        assert_eq!((c.timer, c.px()), (0, (0x10, 0x90)));
    }

    #[test]
    fn enough_coins_already_goes_straight_to_collected() {
        let mut c = coin();
        let mut p = player_at(112, 297, Form::Big);
        let mut frozen = false;
        for _ in 0..400 {
            c.step(&Context { frozen, ..ctx(50, 44, false) }, &mut p);
            frozen = c.freezes();
        }
        assert_eq!((c.phase(), c.sub()), (Phase::Collected, 2));
    }

    #[test]
    fn the_collected_coin_falls_away_when_the_player_dies() {
        let mut c = coin();
        c.phase = 5;
        c.x = HOME.0 << 8;
        c.y = HOME.1 << 8;
        let mut p = Player::spawn(0, 0);
        let dying = |vy| Context { dying: Some(vy), ..ctx(44, 44, true) };
        c.step(&dying(DEATH_HOP), &mut p);
        assert_eq!(c.px().1, 0x90, "waits for the death hop");
        c.step(&dying(DEATH_HOP + 2), &mut p);
        for _ in 0..60 {
            c.step(&dying(0), &mut p);
        }
        assert_eq!(c.px().1, LEAVE_BOTTOM);
    }

    #[test]
    fn draw_and_solid_windows() {
        let c = coin();
        assert!(c.drawn((112 + 0x20, 0)), "drawn up to 0x20 px off the left edge");
        assert!(!c.drawn((112 + 0x21, 0)));
        assert!(c.drawn((112 - 0xFF, 0)));
        assert!(!c.solid((112 - 0xE0, 288 - 0x20)), "solid only within 0x10 px of the edges");
        assert!(c.solid((112 - 0xDF, 288 - 0x20)));
    }
}
