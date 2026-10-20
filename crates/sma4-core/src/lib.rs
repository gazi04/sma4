//! Deterministic game logic. No Bevy, no floats, no wall-clock time: the same input
//! sequence must always produce the same state so it can be diffed against an emulator.

pub mod blocks;
pub mod coins;
pub mod dust;
pub mod enemies;
pub mod goal;
pub mod input;
pub mod level;
pub mod pipes;
pub mod player;
pub mod screens;
pub mod world_map;
pub mod status;

pub use blocks::{Bump, CoinPop, HitOutcome, Item, ItemPhase};
pub use coins::Sparkle;
pub use enemies::{Contact, Enemy, EnemyState, Spawner};
pub use goal::{EndCoin, Goal};
pub use input::Buttons;
pub use level::Level;
pub use pipes::{AreaRequest, Pipe};
pub use player::{Form, Player};
pub use screens::{Dying, GameOver, GameOverMenu, MenuChoice, Screen};
pub use status::{ScorePopup, Status};
pub use world_map::{MapPhase, WorldMap};

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
    pub level: Level,
    pub player: Player,
    /// Where the player respawns after falling out of the level, in pixels.
    pub spawn: (i32, i32),
    /// Blocks currently bouncing. The ROM has three slots for these.
    pub bumps: Vec<Bump>,
    pub items: Vec<Item>,
    pub coin_pops: Vec<CoinPop>,
    /// Effects left by collected level coins.
    pub sparkles: Vec<Sparkle>,
    /// Puffs behind a skidding player.
    pub skid_dust: dust::SkidDust,
    /// World, lives, clock, coins, score and score popups.
    pub status: Status,
    pub enemies: Vec<Enemy>,
    /// Fireballs from Venus Fire Traps (projectile slots 0x03007848 + 0x8EC).
    pub fireballs: Vec<enemies::Fireball>,
    /// Micro-Goombas from the 0x74 Paragoomba (the same projectile slots).
    pub micro_goombas: Vec<enemies::MicroGoomba>,
    pub spawner: Spawner,
    /// Frames of invulnerability left after taking a hit.
    pub invulnerable: u16,
    /// Status-bar "P" blink counter (0x03003852), counted down while the P-meter is full.
    pub p_blink: u8,
    /// The level's clock in seconds (0 = none).
    pub time_limit: u16,
    /// Map, level or game over.
    pub screen: Screen,
    /// The death sequence, while it runs.
    pub dying: Option<Dying>,
    /// The level as entered, to restart it after a lost life.
    start: Option<(Level, Vec<sma4_assets::EnemySpawn>)>,
    /// Bumped whenever the level is restored, so views can redraw it all.
    pub level_generation: u32,
    /// The goal card from the enemy list: where it waits to be spawned, then the object.
    goal_spawn: Option<(i32, i32)>,
    pub goal: Option<Goal>,
    /// Course clear: the camera, frozen when the card was touched (0x03003D0F bit 7 stops
    /// the scroll update 0x08026C8C).
    pub goal_camera: Option<(i32, i32)>,
    /// Coins the enemies turned into at the goal, and how many have paid out (chain).
    pub end_coins: Vec<EndCoin>,
    end_coin_chain: u16,
    /// The world map, when its assets are there; without it the map screen just waits for
    /// A / Start and restarts the last level.
    pub world_map: Option<WorldMap>,
    /// A spot entered on the map: the host loads its level and calls `play`.
    pub pending_level: Option<sma4_assets::MapNode>,
    /// The player in a pipe (drawn behind the tiles meanwhile); everything else waits.
    pub pipe: Option<Pipe>,
    /// A pipe led to the other area: the host loads it and calls `enter_area`.
    pub pending_area: Option<AreaRequest>,
}

// TODO(re): the ROM's post-hit invulnerability length (player flashing) is not decoded yet.
const INVULNERABLE_FRAMES: u16 = 120;

/// Bump-block slots (0x03007848 + 0x690 / 0x700 / 0x770).
const MAX_BUMPS: usize = 3;

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

    /// Status-bar P-meter (0x08003BEC): six arrows lit by meter bits 0-5, and the "P",
    /// lit while the meter is full on frames where the blink counter has bit 3 set.
    pub fn p_meter_display(&self) -> ([bool; 6], bool) {
        let m = self.player.p_meter;
        let arrows = std::array::from_fn(|i| m >> i & 1 != 0);
        (arrows, m & 0x40 != 0 && self.p_blink & 8 != 0)
    }

    pub fn enter_level(&mut self, level: Level, spawn: (i32, i32), enemies: &[sma4_assets::EnemySpawn]) {
        self.start = Some((level.clone(), enemies.to_vec()));
        self.dying = None;
        self.level = level;
        self.spawner = Spawner::new(enemies);
        self.enemies.clear();
        self.fireballs.clear();
        self.micro_goombas.clear();
        self.invulnerable = 0;
        self.spawn = spawn;
        self.player = Player::spawn(spawn.0, spawn.1);
        self.bumps.clear();
        self.items.clear();
        self.coin_pops.clear();
        self.sparkles.clear();
        self.skid_dust = dust::SkidDust::default();
        self.goal_spawn = enemies.iter().find(|e| e.kind == goal::GOAL).map(|e| goal::spawn_px(e.col, e.row));
        self.goal = None;
        self.goal_camera = None;
        self.end_coins.clear();
        self.end_coin_chain = 0;
        self.pipe = None;
        self.pending_area = None;
    }

    /// Arrives through a pipe in the other area of the level (0x08005690): its objects
    /// and enemies, the player coming out of the pipe the pointer names. The clock, the
    /// form and the level to restart after a lost life stay as they were.
    pub fn enter_area(&mut self, level: Level, enemies: &[sma4_assets::EnemySpawn], to: &AreaRequest) {
        let (start, spawn, player) = (self.start.take(), self.spawn, self.player);
        self.enter_level(level, spawn, enemies);
        self.start = start;
        self.player = player;
        // The camera the ROM sets on arrival (0x08005290); a room with fixed camera limits
        // keeps it.
        if self.level.camera_x.is_some() {
            self.level.camera_y = Some(to.exit.camera_y as i32);
        }
        pipes::place(&mut self.player, &to.exit);
        self.pipe = Some(Pipe::exit(to.exit.dir));
        self.level_generation = self.level_generation.wrapping_add(1);
    }

    /// Top-left of a GBA-sized view following the player, clamped to the level (the view
    /// `sma4-bevy` centres on the player).
    pub fn camera(&self) -> (i32, i32) {
        let (cx, cy) = self.player.centre();
        let (w, h) = (SCREEN_W as i32, SCREEN_H as i32);
        let (min_x, max_x) = self.level.camera_x.unwrap_or((0, (self.level.pixel_width() - w).max(0)));
        let x = (cx - w / 2).clamp(min_x, max_x);
        let y = match self.level.camera_y {
            Some(y) => y,
            None => (cy - h / 2).clamp(0, (self.level.pixel_height() - h).max(0)),
        };
        (x, y)
    }

    /// The player takes a hit: big shrinks to small, small dies.
    pub fn hurt(&mut self) {
        if self.invulnerable > 0 || self.dying.is_some() || self.player.transforming() {
            return;
        }
        if self.player.hurt() {
            self.invulnerable = INVULNERABLE_FRAMES;
        } else {
            self.die();
        }
    }

    /// Starts the death sequence (0x0804A474); everything else stops meanwhile.
    pub fn die(&mut self) {
        if self.dying.is_none() {
            self.player.vx = 0;
            self.player.vy = 0;
            self.dying = Some(Dying::new());
        }
    }

    /// Spawns the goal card when the player comes near and runs it; touching it starts the
    /// course clear (0x080684D4).
    fn step_goal(&mut self) {
        if let Some((x, y)) = self.goal_spawn
            && (x + 8 - self.player.centre().0).abs() <= enemies::SPAWN_DISTANCE
        {
            self.goal = Some(Goal::spawn(x, y, &self.player, self.frame));
            self.goal_spawn = None;
        }
        let camera = self.camera();
        let Some(g) = &mut self.goal else { return };
        if !g.step(&self.player, camera, &mut self.status.clock, self.frame).touched {
            return;
        }
        let card = g.card_id();
        self.goal_camera = Some(camera);
        // Stomp chain cleared (player +0x5A = 0); the coins count their own chain.
        self.player.stomp_chain = 0;
        self.end_coin_chain = 0;
        self.status.add_card(card);
        // 0x0806833C: every other object becomes a puff of smoke and a coin.
        for e in &mut self.enemies {
            if e.end_level() {
                let (x, y) = e.px();
                self.end_coins.push(EndCoin::new(x, y));
            }
        }
        for i in self.items.drain(..) {
            let (x, y) = i.px();
            self.end_coins.push(EndCoin::new(x, y));
        }
    }

    /// One frame of the course clear: the player walks off, coins pay out, the goal runs
    /// its texts and clock tally; then back to the map.
    fn step_course_clear(&mut self, camera: (i32, i32)) {
        let stop = Goal::player_stopped(&self.player, camera);
        self.player.goal_walk(&self.level, self.frame, stop);
        self.skid_dust.step(&self.player);
        let (level, player, frame) = (&mut self.level, &self.player, self.frame);
        self.bumps.retain_mut(|b| !b.step(level, player).1);
        let level = &*level;
        self.enemies.retain_mut(|e| e.step(level, player, frame));
        self.coin_pops.retain_mut(|c| c.step());
        self.sparkles.retain_mut(|s| s.step(frame));
        let mut paid = Vec::new();
        self.end_coins.retain_mut(|c| {
            let alive = c.step();
            if !alive {
                paid.push(c.px());
            }
            alive
        });
        for (x, y) in paid {
            self.status.add_coin();
            let kind = (goal::COIN_POPUP + self.end_coin_chain).min(status::POPUP_1UP as u16) as u8;
            self.status.popup(x, y, kind);
            self.end_coin_chain += 1;
        }
        let mut leave = false;
        if let Some(g) = &mut self.goal {
            let ev = g.step(&self.player, camera, &mut self.status.clock, self.frame);
            self.status.add_score(ev.points);
            leave = ev.leave;
        }
        self.status.step_popups(self.frame);
        if leave {
            self.finish_level();
        }
    }

    /// Sub-state 7 done (0x08068160): a full row of cards is used up, three of a kind
    /// giving extra lives (0x080680C4); back to the map.
    fn finish_level(&mut self) {
        if self.status.cards.iter().all(|&c| c != 0) {
            self.status.lives += goal::bonus_lives(&self.status.cards);
            self.status.cards = [0; 3];
        }
        self.goal = None;
        self.goal_camera = None;
        self.end_coins.clear();
        self.status.popups.clear();
        // The spot is cleared (0x02001420 bit, 0x08009428 redraws it); no panel this time.
        if let Some(map) = &mut self.world_map {
            map.clear_here();
        }
        self.to_map(false);
    }

    /// After a death (0x080270B8): one life less, back to the map small; with no life
    /// left (lives & 0x8000), the game-over screen.
    fn lose_life(&mut self) {
        self.dying = None;
        self.player.set_form(Form::Small);
        // Lives go below zero (0xFFFF: the bars show 0) on game over.
        self.status.lives = self.status.lives.wrapping_sub(1);
        if self.status.lives == status::NO_LIVES {
            if let Some(map) = &mut self.world_map {
                map.game_over();
            }
            self.screen = Screen::GameOver(GameOver::new());
        } else {
            self.to_map(true);
        }
    }

    /// Enters the level again from the map: the level as it was first entered, a fresh
    /// clock, the player at the start in its current form.
    pub fn restart_level(&mut self) {
        if let Some((level, enemies)) = self.start.clone() {
            let form = self.player.form;
            self.enter_level(level, self.spawn, &enemies);
            self.player.set_form(form);
        }
        self.status.popups.clear();
        self.status.start_clock(self.time_limit);
        self.level_generation = self.level_generation.wrapping_add(1);
        self.screen = Screen::Level;
    }

    /// Starts a level chosen on the map: the player at `spawn`, a fresh clock.
    pub fn play(&mut self, level: Level, spawn: (i32, i32), enemies: &[sma4_assets::EnemySpawn], time_limit: u16) {
        let form = self.player.form;
        self.enter_level(level, spawn, enemies);
        self.player.set_form(form);
        self.status.popups.clear();
        self.set_time_limit(time_limit);
        self.level_generation = self.level_generation.wrapping_add(1);
        self.screen = Screen::Level;
    }

    /// Back to the map: with the "WORLD n" panel or straight to walking.
    fn to_map(&mut self, intro: bool) {
        if let Some(map) = &mut self.world_map {
            map.show(intro);
        }
        self.screen = Screen::Map;
    }

    /// Starts the level clock (seconds; 0 = no clock).
    pub fn set_time_limit(&mut self, seconds: u16) {
        self.time_limit = seconds;
        self.status.start_clock(seconds);
    }

    /// Advances one frame.
    pub fn step(&mut self, held: Buttons) {
        self.input.pressed = held & !self.input.held;
        self.input.held = held;
        self.frame = self.frame.wrapping_add(1);
        match &mut self.screen {
            Screen::Map => {
                if let Some(map) = &mut self.world_map {
                    if let Some(node) = map.step(held, self.input.pressed) {
                        self.pending_level = Some(node);
                    }
                } else if self.input.pressed.contains(Buttons::A) || self.input.pressed.contains(Buttons::START) {
                    self.restart_level();
                }
                return;
            }
            Screen::GameOver(g) => {
                if !g.step() {
                    self.screen = Screen::GameOverMenu(GameOverMenu::new());
                }
                return;
            }
            Screen::GameOverMenu(menu) => {
                let choice = menu.step(self.input.pressed);
                // 0x080131EC: Mario flies back to the checkpoint.
                if let (Some(k), Some(map)) = (menu.flight(), &mut self.world_map) {
                    map.fly(k);
                }
                match choice {
                    // The window closes and Mario flies back (menu handlers 2-8).
                    Some(MenuChoice::Continue) if self.world_map.is_some() => {}
                    // 0x08012BA8: lives back to 4, on to the map.
                    Some(MenuChoice::Continue | MenuChoice::Continued) => {
                        self.status.lives = status::START_LIVES;
                        if let Some(map) = &mut self.world_map {
                            map.continue_game();
                        }
                        self.to_map(true);
                    }
                    // Quit goes to the title screen in the ROM; there is none here yet, so
                    // a new game starts on the map.
                    Some(MenuChoice::Quit) => {
                        let world = self.status.world;
                        self.status = Status::default();
                        self.status.world = world;
                        if let Some(map) = &mut self.world_map {
                            map.reset();
                        }
                        self.screen = Screen::Map;
                    }
                    None => {}
                }
                return;
            }
            Screen::Level => {}
        }
        if self.level.width == 0 {
            return;
        }
        if let Some(d) = &mut self.dying {
            match d.step() {
                Some(dy) => self.player.y += dy,
                None => self.lose_life(),
            }
            return;
        }

        if let Some(camera) = self.goal_camera {
            self.step_course_clear(camera);
            return;
        }

        // In a pipe: only the slide runs (the player is busy, 0x08094BD8, so the clock
        // holds too); at the end of the way in, the host switches the area.
        if let Some(pipe) = self.pipe {
            self.pipe = pipe.step(&mut self.player);
            if pipe != Pipe::Switching && self.pipe == Some(Pipe::Switching) {
                self.pending_area = pipes::destination(&self.level, &self.player);
                if self.pending_area.is_none() {
                    self.pipe = None;
                }
            }
            return;
        }

        let head_hit = self.player.step(held, self.input.pressed, &self.level, self.frame);
        self.skid_dust.step(&self.player);
        if !self.player.transforming()
            && let Some(dir) = pipes::entry(&self.level, &self.player, held)
        {
            self.pipe = Some(Pipe::enter(dir));
            return;
        }
        if self.player.p_meter & 0x40 != 0 {
            self.p_blink = self.p_blink.wrapping_sub(1);
        }
        // Blocks react the same to a head hit and to the raccoon's tail (0x0809A748).
        for (tx, ty) in head_hit.into_iter().chain(self.player.tail_block()) {
            if self.bumps.len() < MAX_BUMPS
                && let Some((bump, outcome)) = blocks::hit_block(&mut self.level, &self.player, tx, ty)
            {
                if outcome == HitOutcome::Coin {
                    self.status.add_coin();
                    self.coin_pops.push(CoinPop::new(tx, ty));
                }
                self.bumps.push(bump);
            }
        }
        // Level coins: one coin and 50 points each (0x08094780).
        for (tx, ty) in coins::collect(&mut self.level, &self.player, self.frame) {
            self.status.add_coin();
            self.status.add_score(5);
            self.sparkles.push(Sparkle::new(tx, ty));
        }

        let (level, player) = (&mut self.level, &self.player);
        let mut spawned = Vec::new();
        self.bumps.retain_mut(|b| {
            let (item, done) = b.step(level, player);
            spawned.extend(item);
            !done
        });
        self.items.extend(spawned);
        self.items.retain_mut(|i| i.step(level));
        // A coin from a block ends in a 100-points popup (0x080934B4, type 5).
        let status = &mut self.status;
        self.coin_pops.retain_mut(|c| {
            let alive = c.step();
            if !alive {
                status.popup(c.x, c.y, 5);
            }
            alive
        });
        let frame = self.frame;
        self.sparkles.retain_mut(|s| s.step(frame));

        // Touching an item: the mushroom makes a small player big, the leaf gives the
        // raccoon form (power-up request +0x50, form = request - 1).
        // Power-ups give 1000 points (0x0806AF68, popup type 9).
        let mut taken = Vec::new();
        self.items.retain(|i| {
            let t = i.touches(player);
            if t {
                taken.push((i.kind, i.px()));
            }
            !t
        });
        for (kind, (x, y)) in taken {
            if kind == blocks::LEAF { self.player.leaf() } else { self.player.grow() }
            self.status.popup(x, y, 9);
        }

        self.invulnerable = self.invulnerable.saturating_sub(1);
        self.spawner.spawn_near(&self.player, &mut self.enemies);
        // 0x08055B34: past World 1, fire plants spit twice per appearance.
        let twice = self.status.world != 0;
        let (level, player, frame) = (&self.level, &self.player, self.frame);
        self.enemies.retain_mut(|e| {
            e.spits_twice = twice;
            e.step(level, player, frame)
        });
        // 0x08055938 takes a free slot among the first four, or does not spit; 0x0808C8FC
        // gives a Micro-Goomba any free slot.
        for e in &mut self.enemies {
            let used = self.fireballs.len() + self.micro_goombas.len();
            if let Some(f) = e.spit.take()
                && self.fireballs.len() < enemies::FIREBALL_SLOTS
                && used < enemies::PROJECTILE_SLOTS
            {
                self.fireballs.push(f);
            }
            if let Some(m) = e.dropped.take()
                && used < enemies::PROJECTILE_SLOTS
            {
                self.micro_goombas.push(m);
            }
        }
        let camera = self.camera();
        self.fireballs.retain_mut(|f| f.step(camera));
        // 0x0808FBD0 counts the Micro-Goombas riding the player (0x03007820 + 7) for the
        // next frame's speed limit (0x08096884).
        let (player, pressed) = (&self.player, self.input.pressed.bits());
        let can_stick = self.invulnerable == 0 && !player.transforming();
        self.micro_goombas.retain_mut(|m| m.step(player, camera, frame, pressed, can_stick));
        self.player.stuck = self.micro_goombas.iter().filter(|m| m.stuck()).count() as u8;
        for ((x, y), chain) in enemies::shell_hits(&mut self.enemies, frame) {
            self.status.chain_popup(x, y, chain);
        }
        // Sliding shells bump blocks they run into, like the player's head (0x08042EF8).
        let walls: Vec<(i32, i32)> = self.enemies.iter().filter_map(|e| e.wall_hit).collect();
        for (tx, ty) in walls {
            if self.bumps.len() < MAX_BUMPS
                && let Some((bump, outcome)) = blocks::hit_block(&mut self.level, &self.player, tx, ty)
            {
                if outcome == HitOutcome::Coin {
                    self.status.add_coin();
                    self.coin_pops.push(CoinPop::new(tx, ty));
                }
                self.bumps.push(bump);
            }
        }
        // Tail hits score with the stomp chain without advancing it (0x0804955C).
        if let Some(tail) = self.player.tail_box() {
            for e in &mut self.enemies {
                if e.tail_hit(tail, &self.player) {
                    let (x, y) = e.px();
                    self.status.chain_popup(x, y, self.player.stomp_chain);
                }
            }
        }
        let mut hurt = false;
        for e in &mut self.enemies {
            match e.contact(&self.player) {
                // Stomps (0x08048B3C) and kicks from above (0x08048E58) score the chain.
                Contact::Stomped => {
                    self.player.vy = enemies::STOMP_BOUNCE;
                    self.player.on_ground = false;
                    let (x, y) = e.px();
                    self.status.chain_popup(x, y, self.player.stomp_chain);
                    self.player.stomp_chain += 1;
                }
                Contact::KickedFromAbove => {
                    let (x, y) = e.px();
                    self.status.chain_popup(x, y, self.player.stomp_chain);
                    self.player.stomp_chain += 1;
                }
                Contact::HurtPlayer => hurt = true,
                Contact::Kicked | Contact::None => {}
            }
        }
        // 0x0808CC9C -> 0x0808CAEC: a fireball hurts like an enemy (0x0804A5F4) and flies on.
        if self.fireballs.iter().any(|f| f.touches(&self.player)) {
            hurt = true;
        }
        if hurt {
            self.hurt();
        }

        self.step_goal();
        self.status.step_popups(self.frame);
        // Clock (0x080044B8); running out of time kills the player.
        // 0x080044B8 holds the clock while the player is busy (0x03003D08), e.g. growing.
        if !self.player.transforming() && self.status.tick_clock() {
            self.die();
        }
        // Falling out of the level costs a life straight away.
        if self.player.px().1 > self.level.pixel_height() + 32 {
            self.lose_life();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::{SKY, TEST_SOLIDITY, TILE_PX};
    use sma4_assets::{BlockRules, TileKind};

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

    /// Full loop: jump into a question block, wait for the item, walk into it, grow.
    #[test]
    fn hitting_question_block_and_collecting_item_makes_player_big() {
        let (w, h) = (24, 10);
        let mut m = vec![SKY; w * h];
        for x in 0..w {
            m[(h - 1) * w + x] = 0x53;
        }
        let block = (4, 5);
        m[block.1 * w + block.0] = 0x61;
        let mut rules = BlockRules::default();
        rules.bump_behaviour[0x61] = 2;
        rules.result_hi[0x61] = 0x20;
        rules.item_types[5] = blocks::GROWTH_ITEM;
        rules.final_kind[2] = 8;
        rules.final_metatile[7] = 0x5F;
        rules.bounce = [0, -64, -64, -48, -32, -16, 0, 16, 32, 48, 64];

        let mut g = Game::new();
        // Spawn on the ground under the block.
        let ground = (h as i32 - 1) * TILE_PX - 16;
        g.enter_level(Level::new(w, h, m, TEST_SOLIDITY, rules), (block.0 as i32 * TILE_PX + 2, ground), &[]);
        for _ in 0..60 {
            g.step(Buttons::empty());
        }
        assert_eq!(g.player.form, Form::Small);
        g.step(Buttons::A);
        for _ in 0..30 {
            g.step(Buttons::A);
        }
        assert_eq!(g.level.metatile(4, 5), Some(0x5F), "block used up");
        assert_eq!(g.items.len(), 1);
        // The item moves away from the player (0x0804E08C); here the player stands right of
        // the block's left edge, so it goes left. Follow it.
        assert!(g.items[0].vx < 0);
        for _ in 0..400 {
            g.step(Buttons::LEFT);
            if g.player.form == Form::Big {
                break;
            }
        }
        assert_eq!(g.player.form, Form::Big);
        assert!(g.items.is_empty());
        g.hurt();
        assert_eq!(g.player.form, Form::Big, "no hits while growing");
        for _ in 0..player::MORPH_FRAMES {
            g.step(Buttons::empty());
        }

        g.hurt();
        assert_eq!(g.player.form, Form::Small, "the extra hit point absorbs a hit");
    }

    /// Flat level with a question block, using the ROM's tileset-1 rules.
    #[test]
    fn a_micro_goomba_on_the_player_holds_its_speed_down_until_shaken_off() {
        let mut g = Game::new();
        g.enter_level(block_level((40, 2)), (2 * TILE_PX, 9 * TILE_PX - 16), &[]);
        g.step(Buttons::empty());
        let (fx, fy) = g.player.frame();
        // Dropped right onto the player.
        g.micro_goombas = vec![enemies::MicroGoomba::new((fx + 4) * 256, (fy + 14) * 256)];
        let run = Buttons::RIGHT | Buttons::B;
        for _ in 0..90 {
            g.step(run);
        }
        assert_eq!(g.player.stuck, 1);
        assert_eq!(g.player.vx / 16, 0x16, "0x08172C8A[1]: below the walk speed, no running");
        assert_eq!(g.player.p_meter, 0, "no P-meter while carrying it");
        // Each new press of a button loosens it (it tightens again every 16 frames): five
        // and then some shake it off.
        for _ in 0..8 {
            g.step(run | Buttons::A);
            g.step(run);
        }
        assert_eq!(g.player.stuck, 0);
        for _ in 0..60 {
            g.step(run);
        }
        assert!(g.player.vx / 16 > 0x18, "runs again: {:#x}", g.player.vx / 16);
    }

    fn block_level(block: (usize, usize)) -> Level {
        let (w, h) = (24, 10);
        let mut m = vec![SKY; w * h];
        for x in 0..w {
            m[(h - 1) * w + x] = 0x53;
        }
        m[block.1 * w + block.0] = 0x61;
        let mut rules = BlockRules::default();
        rules.bump_behaviour[0x61] = 2;
        rules.result_hi[0x61] = 0x20;
        rules.item_types[5] = blocks::GROWTH_ITEM;
        rules.item_types[3] = blocks::LEAF;
        rules.final_kind[2] = 8;
        rules.final_metatile[7] = 0x5F;
        rules.bounce = [0, -64, -64, -48, -32, -16, 0, 16, 32, 48, 64];
        Level::new(w, h, m, TEST_SOLIDITY, rules)
    }

    /// A big player gets a Super Leaf; it pops up, sways down and makes the player a raccoon.
    #[test]
    fn big_player_gets_a_leaf_that_sways_down_and_makes_a_raccoon() {
        let block = (4, 5);
        let mut g = Game::new();
        let ground = 9 * TILE_PX - 16;
        g.enter_level(block_level(block), (block.0 as i32 * TILE_PX + 2, ground), &[]);
        g.player.set_form(Form::Big);
        for _ in 0..60 {
            g.step(Buttons::empty());
        }
        for _ in 0..12 {
            g.step(Buttons::A);
        }
        assert_eq!(g.items.len(), 1);
        assert_eq!(g.items[0].kind, blocks::LEAF);
        let start_y = block.1 as i32 * TILE_PX;
        let mut highest = g.items[0].px().1;
        let mut xs = Vec::new();
        for _ in 0..120 {
            g.step(Buttons::empty());
            if let Some(i) = g.items.first() {
                highest = highest.min(i.px().1);
                xs.push(i.px().0);
            }
        }
        assert!(start_y - highest >= 30, "pops up about 33 px, got {}", start_y - highest);
        let (lo, hi) = (*xs.iter().min().unwrap(), *xs.iter().max().unwrap());
        assert!(hi - lo >= 16, "sways from side to side");
        // It falls onto the player standing under the block.
        for _ in 0..200 {
            if g.player.form == Form::Raccoon {
                break;
            }
            let leaf_x = g.items.first().map_or(0, |i| i.px().0);
            let dir = if leaf_x > g.player.px().0 { Buttons::RIGHT } else { Buttons::LEFT };
            g.step(dir);
        }
        assert_eq!(g.player.form, Form::Raccoon);
        assert!(g.items.is_empty());
    }

    /// The raccoon's tail bumps a question block beside it (0x0809A748).
    #[test]
    fn tail_whip_bumps_a_block_beside_the_player() {
        // Block on the ground row level with the tail probe (24 px below the frame top), in
        // front of the player: by whip frame 9 the spin has turned the player around, so
        // the probe (x + 22 facing left) points the way the player started facing.
        let block = (8, 8);
        let mut g = Game::new();
        let ground = 9 * TILE_PX - 26;
        g.enter_level(block_level(block), (7 * TILE_PX + 4, ground), &[]);
        g.player.set_form(Form::Raccoon);
        g.player.facing_left = false;
        for _ in 0..30 {
            g.step(Buttons::empty());
        }
        g.step(Buttons::B);
        for _ in 0..20 {
            g.step(Buttons::B);
        }
        assert_eq!(g.level.metatile(8, 8), Some(0x5F), "block used up");
        assert_eq!(g.items.len(), 1, "it released its item");
    }

    #[test]
    fn tail_whip_knocks_out_goombas_and_flips_koopas() {
        let mut g = Game::new();
        let ground = 9 * TILE_PX - 26;
        g.enter_level(block_level((20, 2)), (8 * TILE_PX, ground), &[]);
        g.player.set_form(Form::Raccoon);
        g.player.facing_left = true;
        for _ in 0..30 {
            g.step(Buttons::empty());
        }
        let p = g.player;
        let place = |kind, x: i32| {
            let mut e = enemies::Enemy::spawn_at(kind, x, 8 * TILE_PX, &p);
            e.vx = 0;
            e
        };
        // Tail box covers x + 17..+29 (facing left) and x - 12..x (facing right).
        let fx = p.centre().0 - 8;
        g.enemies = vec![place(enemies::WALKER, fx + 18), place(enemies::KOOPA, fx - 14)];
        g.step(Buttons::B);
        for _ in 0..18 {
            g.step(Buttons::empty());
        }
        assert_eq!(g.enemies[0].state, EnemyState::Dead);
        assert!(matches!(g.enemies[1].state, EnemyState::Shell(_)) && g.enemies[1].flipped);
    }

    #[test]
    fn map_spots_lead_into_levels_and_clearing_one_changes_its_tile() {
        let mut g = Game::new();
        let mut map = world_map::tests::world1();
        map.show(false);
        g.world_map = Some(map);
        g.screen = Screen::Map;
        let tap = |g: &mut Game, b: Buttons| {
            g.step(Buttons::empty());
            g.step(b);
            for _ in 0..20 {
                g.step(b);
            }
        };
        tap(&mut g, Buttons::RIGHT);
        tap(&mut g, Buttons::UP);
        tap(&mut g, Buttons::A);
        for _ in 0..60 {
            g.step(Buttons::empty());
        }
        let node = g.pending_level.take().expect("entered panel 1");
        assert_eq!(node.level, 0x22);
        // The host loads the level and plays it; the goal is touched and the level ends.
        let (w, h) = (40, 10);
        let mut kinds = vec![TileKind::Empty; w * h];
        for x in 0..w {
            kinds[(h - 1) * w + x] = TileKind::Solid;
        }
        let list = [sma4_assets::EnemySpawn { kind: goal::GOAL, col: 6, row: 8 }];
        g.play(Level::from_kinds(w, h, &kinds), (6 * TILE_PX, 9 * TILE_PX - 16), &list, 0);
        assert_eq!(g.screen, Screen::Level);
        let mut frames = 0;
        while g.screen == Screen::Level {
            g.step(Buttons::empty());
            frames += 1;
            assert!(frames < 2000);
        }
        let map = g.world_map.as_ref().unwrap();
        assert_eq!(map.phase, MapPhase::Idle, "no panel after a clear");
        assert_eq!(map.tile(4, 1), 0x00, "panel 1 is done");
        assert_eq!((map.y, map.x), (0x20, 0x40));
        // A lost life brings the panel back.
        g.play(Level::from_kinds(w, h, &kinds), (6 * TILE_PX, 9 * TILE_PX - 16), &[], 0);
        g.die();
        while g.screen == Screen::Level {
            g.step(Buttons::empty());
        }
        assert!(matches!(g.world_map.as_ref().unwrap().phase, MapPhase::Intro(_)));
    }

    #[test]
    fn touching_the_goal_card_clears_the_level() {
        let (w, h) = (40, 10);
        let mut kinds = vec![TileKind::Empty; w * h];
        for x in 0..w {
            kinds[(h - 1) * w + x] = TileKind::Solid;
        }
        let mut g = Game::new();
        let ground = 9 * TILE_PX - 16;
        let list = [sma4_assets::EnemySpawn { kind: goal::GOAL, col: 6, row: 8 }];
        g.enter_level(Level::from_kinds(w, h, &kinds), (6 * TILE_PX, ground), &list);
        g.set_time_limit(100);
        g.screen = Screen::Level;
        g.step(Buttons::empty());
        assert!(g.goal_camera.is_some(), "touched");
        let card = g.status.cards[0];
        assert!((1..=3).contains(&card));
        let x0 = g.player.px().0;
        let mut frames = 0;
        while g.screen == Screen::Level {
            // Input is ignored meanwhile.
            g.step(Buttons::LEFT | Buttons::A);
            frames += 1;
            assert!(frames < 2000, "never left the level");
        }
        assert_eq!(g.screen, Screen::Map);
        assert!(g.player.px().0 > x0 + 100, "walked off to the right");
        assert_eq!(g.status.seconds(), 0);
        assert_eq!(g.status.score, 10 * 50, "100 s at 50 points a second");
        assert_eq!(g.status.cards, [card, 0, 0]);
    }

    /// A Piranha Plant all the way out of a pipe whose top is at `y`, left column at `x`.
    fn plant_out(x: i32, y: i32, p: &player::Player) -> enemies::Enemy {
        let mut e = enemies::Enemy::spawn_at(enemies::PIRANHA, x, y, p);
        e.state = EnemyState::Plant { phase: 2, timer: 0x30 };
        e.y = (y - 0x19) * player::SUB;
        e
    }

    #[test]
    fn a_fireball_hurts_the_player_and_flies_on() {
        let mut g = Game::new();
        g.enter_level(block_level((20, 2)), (8 * TILE_PX, 9 * TILE_PX - 16), &[]);
        g.player.grow();
        for _ in 0..player::MORPH_FRAMES {
            g.step(Buttons::empty());
        }
        let (fx, fy) = g.player.frame();
        // Just left of the player, flying right at it.
        let mut b = enemies::Fireball { x: (fx - 6) * 256, y: (fy + 10) * 256, vx: 0x0B, vy: 0, age: 0 };
        b.x -= 0x0B * 16;
        g.fireballs = vec![b];
        g.step(Buttons::empty());
        assert_eq!(g.player.form, Form::Small, "big shrinks");
        assert_eq!(g.fireballs.len(), 1, "the fireball is not used up");
    }

    #[test]
    fn piranha_plant_hurts_on_touch_and_the_tail_puffs_it_away() {
        let mut g = Game::new();
        let ground = 9 * TILE_PX - 26;
        g.enter_level(block_level((20, 2)), (8 * TILE_PX, ground), &[]);
        g.player.set_form(Form::Raccoon);
        g.player.facing_left = true;
        for _ in 0..30 {
            g.step(Buttons::empty());
        }
        let p = g.player;
        // Its box overlaps the player: a hit (raccoon -> big).
        g.enemies = vec![plant_out(p.px().0 - 8, 9 * TILE_PX + 8, &p)];
        g.step(Buttons::empty());
        assert_eq!(g.player.form, Form::Big);

        // Out of reach of the body but inside the tail box (x + 17..+29 facing left).
        let mut g = Game::new();
        g.enter_level(block_level((20, 2)), (8 * TILE_PX, ground), &[]);
        g.player.set_form(Form::Raccoon);
        g.player.facing_left = true;
        for _ in 0..30 {
            g.step(Buttons::empty());
        }
        let p = g.player;
        g.enemies = vec![plant_out(p.centre().0 - 8 + 18 - 8, 9 * TILE_PX + 8, &p)];
        g.step(Buttons::B);
        for _ in 0..18 {
            g.step(Buttons::empty());
        }
        assert!(matches!(g.enemies[0].state, EnemyState::Puff(_)));
        assert_eq!(g.player.form, Form::Raccoon);
        assert_eq!(g.status.score, 10, "100 points");
        for _ in 0..0x1F {
            g.step(Buttons::empty());
        }
        assert!(g.enemies.is_empty(), "the puff is gone");
    }

    #[test]
    fn p_meter_display_lights_arrows_and_blinks_the_p_when_full() {
        let mut g = Game::new();
        g.enter_level(block_level((20, 2)), (16, 9 * TILE_PX - 16), &[]);
        g.player.p_meter = 0b0000111;
        assert_eq!(g.p_meter_display(), ([true, true, true, false, false, false], false));
        g.player.p_meter = player::P_FULL;
        let mut lit = Vec::new();
        for _ in 0..32 {
            g.p_blink = g.p_blink.wrapping_sub(1);
            lit.push(g.p_meter_display().1);
        }
        // 8 frames on, 8 off.
        assert_eq!(lit.iter().filter(|&&l| l).count(), 16);
        assert!(lit.windows(2).filter(|w| w[0] != w[1]).count() >= 3);
    }

    /// A kicked shell sliding into a ? block on the ground bumps it (0x08042EF8).
    #[test]
    fn kicked_shell_bumps_a_block_it_runs_into() {
        let block = (12, 8);
        let mut g = Game::new();
        g.enter_level(block_level(block), (2 * TILE_PX, 9 * TILE_PX - 16), &[]);
        for _ in 0..30 {
            g.step(Buttons::empty());
        }
        let mut shell = enemies::Enemy::spawn_at(enemies::KOOPA, 4 * TILE_PX, 8 * TILE_PX, &g.player);
        shell.state = EnemyState::Shell(0xFF);
        shell.vx = 0;
        g.enemies = vec![shell];
        // Walk into it: it slides right, into the block.
        for _ in 0..90 {
            g.step(Buttons::RIGHT);
            if g.enemies[0].state == EnemyState::Kicked {
                break;
            }
        }
        assert_eq!(g.enemies[0].state, EnemyState::Kicked);
        for _ in 0..60 {
            g.step(Buttons::empty());
        }
        assert_eq!(g.level.metatile(block.0 as i32, block.1 as i32), Some(0x5F), "block used up");
        assert_eq!(g.items.len(), 1, "it released its item");
        assert!(g.enemies[0].vx < 0, "the shell bounced back");
    }

    /// Stomping two enemies without landing scores 100 then 200 (0x08093654 chain).
    #[test]
    fn stomp_chain_scores_100_then_200() {
        let mut g = Game::new();
        let ground = 9 * TILE_PX - 16;
        g.enter_level(block_level((20, 2)), (8 * TILE_PX, ground - 64), &[]);
        let goomba = |g: &Game| {
            let x = g.player.px().0;
            let mut e = enemies::Enemy::spawn_at(enemies::WALKER, x, ground, &g.player);
            e.vx = 0;
            e
        };
        // Falls onto a goomba, bounces; a second one appears under it before it lands.
        g.enemies = vec![goomba(&g)];
        for _ in 0..60 {
            g.step(Buttons::empty());
            if g.player.stomp_chain == 1 {
                break;
            }
        }
        assert_eq!(g.player.stomp_chain, 1);
        let second = goomba(&g);
        g.enemies.push(second);
        for _ in 0..60 {
            g.step(Buttons::empty());
            if g.player.stomp_chain == 2 {
                break;
            }
        }
        assert_eq!(g.player.stomp_chain, 2);
        for _ in 0..10 {
            g.step(Buttons::empty());
        }
        assert_eq!(g.status.score, 10 + 20, "100 + 200 points");
    }

    #[test]
    fn first_jump_on_a_paragoomba_takes_its_wings_the_second_flattens_it() {
        let mut g = Game::new();
        let ground = 9 * TILE_PX - 16;
        g.enter_level(block_level((20, 2)), (8 * TILE_PX, ground - 64), &[]);
        let x = g.player.px().0;
        let mut e = enemies::Enemy::spawn_at(enemies::PARAGOOMBA, x, ground, &g.player);
        e.vx = 0;
        g.enemies = vec![e];
        // Falls onto it: the wings come off and it lives on as a Goomba.
        while g.player.stomp_chain == 0 {
            g.step(Buttons::empty());
        }
        let e = g.enemies[0];
        assert_eq!((e.kind, e.state, e.was_para), (enemies::WALKER, EnemyState::Walking, true));
        // The bounce does not stomp it again (8 frames of contact grace, 0x08048B3C).
        for _ in 0..12 {
            g.step(Buttons::empty());
        }
        assert_eq!((g.player.stomp_chain, g.enemies[0].state), (1, EnemyState::Walking));
        // Coming down on it a second time flattens it.
        while g.player.stomp_chain == 1 {
            g.step(Buttons::empty());
        }
        assert!(matches!(g.enemies[0].state, EnemyState::Squished(_)));
        assert_eq!(g.dying, None);
    }

    #[test]
    fn running_out_of_time_kills_and_returns_to_the_map() {
        let mut g = Game::new();
        g.enter_level(block_level((20, 2)), (8 * TILE_PX, 9 * TILE_PX - 16), &[]);
        g.set_time_limit(1);
        let lives = g.status.lives;
        for _ in 0..60 {
            g.step(Buttons::empty());
        }
        assert!(g.dying.is_some(), "time up starts the death sequence");
        for _ in 0..300 {
            g.step(Buttons::empty());
        }
        assert_eq!(g.screen, Screen::Map);
        assert_eq!(g.status.lives, lives - 1);
        // A from the map enters the level again with a fresh clock.
        g.step(Buttons::A);
        assert_eq!(g.screen, Screen::Level);
        assert_eq!(g.status.seconds(), 1);
    }

    #[test]
    fn losing_the_last_life_shows_game_over_then_a_new_game() {
        let mut g = Game::new();
        g.enter_level(block_level((20, 2)), (8 * TILE_PX, 9 * TILE_PX - 16), &[]);
        g.status.lives = 0;
        g.status.score = 500;
        g.die();
        for _ in 0..300 {
            g.step(Buttons::empty());
        }
        assert!(matches!(g.screen, Screen::GameOver(_)));
        for _ in 0..400 {
            g.step(Buttons::empty());
        }
        assert!(matches!(g.screen, Screen::GameOverMenu(_)));
        assert_eq!(g.status.lives, status::NO_LIVES, "the bar shows 0 lives");
        for _ in 0..20 {
            g.step(Buttons::empty());
        }
        g.step(Buttons::A);
        assert_eq!(g.screen, Screen::Map);
        assert_eq!((g.status.lives, g.status.score), (status::START_LIVES, 500), "continue keeps the score");
    }

    #[test]
    fn continue_closes_the_panel_and_flies_mario_back_to_the_world_start() {
        use crate::screens::{MenuMario, MenuStage};
        let mut g = Game::new();
        let mut map = world_map::tests::world1();
        // Mario entered 1-2 (cleared 1-1 is behind him, but the Toad house spot is done too).
        map.clear_cell(12, 3);
        (map.y, map.x) = (0x20, 0x80);
        let start = map.checkpoint;
        g.world_map = Some(map);
        g.enter_level(block_level((20, 2)), (8 * TILE_PX, 9 * TILE_PX - 16), &[]);
        g.status.lives = 0;
        g.die();
        while !matches!(g.screen, Screen::GameOverMenu(_)) {
            g.step(Buttons::empty());
        }
        for _ in 0..0x10 {
            g.step(Buttons::empty());
        }
        g.step(Buttons::A);
        let menu = |g: &Game| match g.screen {
            Screen::GameOverMenu(m) => m,
            _ => panic!("left the menu early: {:?}", g.screen),
        };
        // The window shrinks 8 px a side per frame; it is gone once its 0x28 half height is
        // (the frame A was pressed on, then 5 frames).
        let mut widths = Vec::new();
        while let Some((_, _, w, _)) = menu(&g).window() {
            widths.push(w);
            g.step(Buttons::empty());
        }
        assert_eq!(widths, [0x78, 0x78, 0x68, 0x58, 0x48, 0x38]);
        // The map stays dim until the half width is 0 too (0x3C: 8 frames in all).
        let mut dim = 0;
        while menu(&g).dimmed() {
            g.step(Buttons::empty());
            dim += 1;
        }
        assert_eq!(dim, 2);
        let mut frames = 0;
        let mut phases = Vec::new();
        let mut path = Vec::new();
        while matches!(menu(&g).stage, MenuStage::Close | MenuStage::Wait(_) | MenuStage::Fly(_)) {
            if let Some(MenuMario::Spin(p)) = menu(&g).mario() {
                phases.push(p);
            }
            let m = g.world_map.as_ref().unwrap();
            path.push((m.y, m.x));
            g.step(Buttons::empty());
            frames += 1;
        }
        assert_eq!(&phases[..8], [3, 3, 2, 2, 1, 1, 0, 0], "Mario spins, 2 frames a side");
        assert_eq!(path.first(), Some(&(0x20, 0x80)), "from where he entered the level");
        let m = g.world_map.as_ref().unwrap();
        assert_eq!((m.y, m.x), start, "...straight back to START");
        assert!(frames >= 0x10 + 0x20);
        assert_eq!(menu(&g).mario(), Some(MenuMario::Normal));
        assert_eq!(g.status.lives, status::NO_LIVES, "4 lives only once back on the map");
        for _ in 0..0x11 {
            g.step(Buttons::empty());
        }
        assert_eq!(g.screen, Screen::Map);
        assert_eq!(g.status.lives, status::START_LIVES);
        let m = g.world_map.as_ref().unwrap();
        assert!(matches!(m.phase, MapPhase::Intro(_)), "with the WORLD 1 panel");
        assert_eq!(m.tile(12, 3), 0x50, "the Toad house is back");
    }
}
