//! Deterministic game logic. No Bevy, no floats, no wall-clock time: the same input
//! sequence must always produce the same state so it can be diffed against an emulator.

pub mod advance_coin;
pub mod airship;
pub mod ending;
pub mod autoscroll;
pub mod blocks;
pub mod boom_boom;
pub mod castle;
pub mod camera_zones;
pub mod crusher;
pub mod doors;
pub mod coins;
pub mod dust;
pub mod enemies;
pub mod fire;
pub mod fortress;
pub mod goal;
pub mod input;
pub mod inventory;
pub mod larry;
pub mod level;
pub mod obj;
pub mod lifts;
pub mod map_objects;
pub mod map_stars;
pub mod pipes;
pub mod player;
pub mod rng;
pub mod screens;
pub mod ship;
pub mod spade;
pub mod title;
pub mod world_map;
pub mod status;
pub mod toad_house;
pub mod warp;
pub mod wand;
pub mod water;

/// Fireball knockouts score 100 (0x08093560 with popup 5); star chains stop growing at
/// the 9th (0x080488E4 caps +0x107 at 8).
const FIRE_POPUP: u8 = 5;
const STAR_CHAIN_MAX: u16 = 8;

pub use blocks::{Bump, CoinPop, HitFrom, HitOutcome, Item, ItemPhase};
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
    /// Pieces of broken bricks (effect slots 0x03007848 + 0x7E0).
    pub debris: Vec<blocks::BrickDebris>,
    /// Fire Mario's fireballs (player projectile slots 0x03007848 + 0xAD4 / + 0xAE8).
    pub player_fireballs: Vec<fire::PlayerFireball>,
    /// The ROM's random bytes (0x03003878), stepped every frame.
    pub rng: rng::Rng,
    /// Boomerang Bros' boomerangs (the same projectile slots).
    pub boomerangs: Vec<enemies::Boomerang>,
    /// Hammer Bros' hammers (the same projectile slots).
    pub hammers: Vec<enemies::Hammer>,
    /// Micro-Goombas from the 0x74 Paragoomba (the same projectile slots).
    pub micro_goombas: Vec<enemies::MicroGoomba>,
    pub spawner: Spawner,
    /// Goomba pipes in range (generator slots 0x03007848 + 0xA54).
    pub goomba_pipes: Vec<enemies::GoombaPipe>,
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
    /// World 1-3's warp trigger (type 0x07), in a level that has one.
    warp_trigger: Option<toad_house::WarpTrigger>,
    /// The trigger fired, or a map Toad House was entered: the host loads this Toad House
    /// and calls `enter_house` (`enter_map_house` when `house_choice` is set).
    pub pending_house: Option<u16>,
    /// A map Toad House's chest choice (0x03003CAF: the enemy word's high byte & 0x7F).
    pub house_choice: Option<u8>,
    /// The house was entered from the map.
    map_house: bool,
    /// In a Toad House.
    pub house: Option<toad_house::ToadHouse>,
    /// The spade panel's game (`sma4_assets::SpadeAsset`); without it the spade is just
    /// left again.
    pub spade_data: Option<sma4_assets::SpadeAsset>,
    pub spade: Option<spade::Spade>,
    /// Its wins in a row (0x03002CD8 / D9).
    pub spade_streak: spade::Streak,
    /// The title screen's graphics, maps and tables (`sma4_assets::TitleAsset`); without
    /// them QUIT starts a new game on the map straight away.
    pub title_data: Option<std::sync::Arc<sma4_assets::TitleAsset>>,
    pub title: Option<title::Title>,
    /// The player is frozen (+0x40, 0x080A8000): only drawn; counts down.
    freeze: u8,
    /// Items got (0x03002C2E, inventory item ids), used from the map's item panel.
    pub inventory: Vec<u8>,
    /// The map's item panel, open (L on the map, `inventory`).
    pub item_panel: Option<inventory::Inventory>,
    /// The Warp Whistle's whirlwind and wave (`warp`).
    pub warp: Option<warp::Warp>,
    /// The whistle reached World 9: the host loads that world's map (0x03002A69) and calls
    /// `enter_world`.
    pub pending_world: Option<u8>,
    /// A Starman used on the map: the next level starts with it (player +0x5C).
    pub pending_star: bool,
    /// Autoscroll scripts from the ROM (`sma4_assets::AutoscrollScripts`); without them a
    /// level's autoscroll never starts.
    pub autoscroll_scripts: Option<sma4_assets::AutoscrollScripts>,
    /// The running autoscroll, which the camera follows.
    pub autoscroll: Option<autoscroll::Autoscroll>,
    /// Falling lifts (World 1-4).
    pub lifts: Vec<lifts::Lift>,
    /// How far the game has been played (0x03002C85): the Advance Coin only appears once
    /// it is not 0 (the game has been beaten).
    pub progress: u8,
    /// The level's Advance Coins, its coin counter (0x030037A0) and quota (0x0300379F).
    pub advance_coins: Vec<advance_coin::AdvanceCoin>,
    pub level_coins: u8,
    pub coin_quota: u8,
    /// An Advance Coin has been hit in this level (0x03007848 + 0xE67).
    pub advance_taken: bool,
    /// Each world's end room (`sma4_assets::read_end_rooms`), where end-of-level pipes lead.
    pub end_rooms: Option<[sma4_assets::EndRoom; sma4_assets::WORLDS]>,
    /// The pipe the player is in is an end-of-level pipe.
    pipe_to_end: bool,
    /// Water splashes (effect slots 0x03007848 + 0x888) and air bubbles (+ 0x858).
    pub splashes: Vec<water::Splash>,
    pub bubbles: Vec<water::Bubble>,
    /// Objects' splashes alternate between the last two slots (0x03007848 + 0xC94).
    splash_turn: u8,
    /// Vertical camera triggers of the enemy list (World 1-5).
    pub camera_zones: camera_zones::CameraZones,
    /// The mini-fortress: Podoboos, Roto-Discs and their effects, Boom Boom and his ball,
    /// the treasure chest, the crushing ceiling.
    pub podoboos: Vec<fortress::Podoboo>,
    pub discs: Vec<fortress::RotoDisc>,
    pub embers: Vec<fortress::Ember>,
    pub lava_splashes: Vec<fortress::LavaSplash>,
    pub sparks: Vec<fortress::Spark>,
    pub puffs: Vec<fortress::Puff>,
    pub boom_boom: Option<boom_boom::BoomBoom>,
    pub ball: Option<boom_boom::Ball>,
    pub chest: Option<fortress::Chest>,
    /// The chest's item (list type 0x1AA, 0x0300379D) and level effect 7 (0x03003CF7).
    chest_item: u8,
    /// Level effect 7's countdown to the end of the level (+0xBE9).
    exit_timer: u8,
    /// Level effect 7 is on (list type 0x166, 0x03003CF7 = 7): a chest comes once the
    /// Bros and their projectiles are gone (0x0804D12C).
    chest_effect: bool,
    /// A map object's battle: the item its chest holds (0x0300379D).
    pub battle: Option<u8>,
    pub crusher: Option<crusher::Crusher>,
    /// Camera x limits set by objects (min: player +0xA0, max: +0x9E).
    camera_limits: (Option<i32>, Option<i32>),
    /// Boom Boom's intro holds the player (+0x40 = 0xFF).
    boss_freeze: bool,
    /// The ball was touched: the clock tally (`goal::tally`) and the frames to the map.
    pub fortress_clear: Option<goal::ClockTally>,
    /// 0x03003D0F set by Boom Boom's burst or the opened chest: the clock (0x080044B8) holds.
    clock_stopped: bool,
    /// The castle's tables and messages (`sma4_assets::CastleAsset`); without them the
    /// throne room is an empty room.
    pub castle_data: Option<std::sync::Arc<sma4_assets::CastleAsset>>,
    /// The King's throne room (list type 0x1A9).
    pub throne: Option<castle::Throne>,
    /// Hardware sprites of the objects whose draw routines are ported as they are, in GBA
    /// screen pixels and OAM order (`obj`); the host draws them from the room's OBJ VRAM.
    pub obj_sprites: Vec<obj::ObjSprite>,
    /// The airship's cannons, cannonballs, smoke and propellers (`ship`).
    pub ship: ship::Ship,
    /// The autoscroll's step this frame (0x03003877 / 0x03003876), px.
    scroll_step: (i32, i32),
    /// The screen between the castle's rooms (`castle::Curtain`), and the room it leads to.
    pub curtain: Option<castle::Curtain>,
    curtain_to: Option<AreaRequest>,
    /// The airship's intro (level 0x202, start kind 6).
    pub intro: Option<airship::Intro>,
    /// The background's scroll when a room sets it itself (GBA screen origin), instead of
    /// following the camera.
    pub bg_scroll: Option<(i32, i32)>,
    /// Thrown up onto the airship's deck (start kind 7, player +0x45 = 4): the speed byte.
    landing: Option<i8>,
    /// Larry Koopa in his cabin (object 0x0E) and his rings (`larry`).
    pub larry: Option<larry::Larry>,
    /// Larry's wand falling in his cabin (projectile 0x10, `wand`).
    pub wand: Option<wand::Wand>,
    /// The wand caught: Larry's states 3-4, "LEVEL CLEARED!" and the clock tally.
    pub wand_clear: Option<wand::Cleared>,
    /// The bumped wand's sparkles (effect 6, `wand::Sparkle`).
    pub wand_sparkles: Vec<wand::Sparkle>,
    /// The tail's hit star (0x03007848 + 0xC03, `wand::HitStar`).
    pub hit_star: Option<wand::HitStar>,
    /// A Tanooki statue's touch of the wand: the burst (0xC08) and the smoke (effect 1).
    pub statue_burst: Option<wand::StatueBurst>,
    pub wand_smoke: Option<ship::Smoke>,
    /// How many of `obj_sprites` go in front of the player (their OAM entries come before
    /// his); the rest go behind.
    pub obj_front: usize,
    /// The level's layers show nothing (their camera is on empty rows: Larry's state 5).
    pub level_hidden: bool,
    /// BG colour 0 (BGR555) when a sequence sets it, instead of the level's backdrop.
    pub backdrop: Option<u16>,
}

/// Autoscroll kind of the crushing ceiling (row >> 4).
const CRUSHER_KIND: u8 = 3;

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
        self.player_fireballs.clear();
        self.boomerangs.clear();
        self.hammers.clear();
        self.debris.clear();
        self.goomba_pipes.clear();
        self.ship = ship::Ship::default();
        self.scroll_step = (0, 0);
        self.player.stuck = 0;
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
        self.warp_trigger = toad_house::WarpTrigger::find(enemies);
        self.pending_house = None;
        self.house = None;
        self.freeze = 0;
        self.autoscroll = None;
        self.lifts.clear();
        // The counter, the quota and the hit flag belong to the level, not the area
        // (0x08017170 clears them on the way back to the map; `enter_area` keeps them).
        self.advance_coins.clear();
        self.level_coins = 0;
        self.coin_quota = 0;
        self.advance_taken = false;
        self.splashes.clear();
        self.bubbles.clear();
        self.podoboos.clear();
        self.discs.clear();
        self.embers.clear();
        self.lava_splashes.clear();
        self.sparks.clear();
        self.puffs.clear();
        self.boom_boom = None;
        self.ball = None;
        self.chest = None;
        self.chest_item = 0;
        self.exit_timer = 0;
        self.chest_effect = false;
        self.crusher = None;
        self.level.shift = None;
        self.camera_limits = (None, None);
        self.boss_freeze = false;
        self.fortress_clear = None;
        self.clock_stopped = false;
        let start_y = self.level.start_camera_y.unwrap_or(0);
        self.camera_zones =
            camera_zones::CameraZones::new(enemies, self.level.camera, start_y, self.level.id, self.player.frame().1);
        self.obj_sprites.clear();
        self.obj_front = 0;
        self.level_hidden = false;
        self.backdrop = None;
        self.larry = None;
        self.wand = None;
        self.wand_clear = None;
        self.wand_sparkles.clear();
        self.hit_star = None;
        self.statue_burst = None;
        self.wand_smoke = None;
        self.intro = None;
        self.landing = None;
        self.bg_scroll = None;
        self.player.forced_pose = None;
        // The throne room (0x0804B768 counts type 0x1A9 into 0x03003D12): the map's
        // airship still at the castle is world slot 0 (0x03002BBE[world * 14]).
        let world = self.status.world.saturating_sub(1);
        let airship = self.world_map.as_ref().is_none_or(|m| m.objects.slots[0].kind != 0);
        self.throne = match &self.castle_data {
            Some(d) if enemies.iter().any(|e| e.kind == castle::THRONE) => Some(castle::Throne::new(d, world, airship)),
            _ => None,
        };
        if let Some(t) = &self.throne {
            self.obj_sprites = t.sprites();
        }
    }

    /// Arrives through a pipe in the other area of the level (0x08005690): its objects
    /// and enemies, the player coming out of the pipe the pointer names. The clock, the
    /// form and the level to restart after a lost life stay as they were.
    pub fn enter_area(&mut self, level: Level, enemies: &[sma4_assets::EnemySpawn], to: &AreaRequest) {
        let (start, spawn, player) = (self.start.take(), self.spawn, self.player);
        let coins = (self.level_coins, self.coin_quota, self.advance_taken);
        self.enter_level(level, spawn, enemies);
        (self.level_coins, self.coin_quota, self.advance_taken) = coins;
        self.start = start;
        self.player = player;
        // The camera the ROM sets on arrival (0x08005290); an autoscroll starting there
        // starts from it.
        self.level.start_camera_y = Some(to.exit.camera_y as i32);
        self.camera_zones.set(to.exit.camera_y as i32);
        pipes::place(&mut self.player, &to.exit);
        // The airship's intro (start kind 6, 0x0809F400) and its deck (kind 7, 0x0809F43C).
        if self.level.start_kind == airship::INTRO_KIND {
            self.pipe = None;
            let anchor = enemies.iter().find(|e| e.kind == airship::ANCHOR).map(|e| (e.col, e.row));
            self.intro = Some(airship::Intro::new(self.player.frame(), anchor));
            self.level_generation = self.level_generation.wrapping_add(1);
            return;
        }
        if self.level.start_kind == airship::LANDING_KIND {
            self.pipe = None;
            // 0x0809F43C: the frame x's low byte 0x90, y = BG camera y + 0x80, speed -0x70.
            let fx = (self.player.frame().0 & !0xFF) | airship::LANDING_X;
            let fy = to.exit.camera_y as i32 + airship::LANDING_DY;
            self.player.set_frame_sub(fx * player::SUB, fy * player::SUB);
            self.player.on_ground = false;
            self.player.vx = 0;
            self.landing = Some(airship::LANDING_SPEED);
            self.player.forced_pose = Some(player::Pose::Jump);
            self.level_generation = self.level_generation.wrapping_add(1);
            return;
        }
        // Through a door (exit direction 8): standing there.
        if to.exit.dir == doors::DOOR_EXIT {
            self.pipe = None;
            self.player.on_ground = true;
        } else if to.exit.dir == pipes::SKY_EXIT {
            self.pipe = None;
            self.player.sky = player::SKY_ARRIVAL;
        } else {
            self.pipe = Some(Pipe::exit(to.exit.dir));
        }
        self.level_generation = self.level_generation.wrapping_add(1);
    }

    /// Warps into a Toad House (0x03003CA0 = 5, 0x0800578C): its objects, the player at
    /// `toad_house::ARRIVAL` in the same form, no clock. `message` is Toad's message
    /// (`sma4_assets::toad_message_path`). The level to restart after a lost life stays.
    pub fn enter_house(&mut self, level: Level, enemies: &[sma4_assets::EnemySpawn], message: Vec<u8>) {
        let arrival = toad_house::ARRIVAL;
        self.enter_house_at(level, enemies, message, arrival, toad_house::HIDDEN_CHOICE);
    }

    /// A Toad House on the map (tileset 7, `pending_house` with `house_choice` set): Mario
    /// at `toad_house::MAP_ARRIVAL`; the spot is cleared when he leaves.
    pub fn enter_map_house(&mut self, level: Level, enemies: &[sma4_assets::EnemySpawn], message: Vec<u8>) {
        let choice = self.house_choice.take().unwrap_or(toad_house::HIDDEN_CHOICE);
        self.enter_house_at(level, enemies, message, toad_house::MAP_ARRIVAL, choice);
        self.map_house = true;
        self.screen = Screen::Level;
    }

    fn enter_house_at(
        &mut self,
        level: Level,
        enemies: &[sma4_assets::EnemySpawn],
        message: Vec<u8>,
        (fx, fy): (i32, i32),
        choice: u8,
    ) {
        let (start, spawn, player) = (self.start.take(), self.spawn, self.player);
        self.enter_level(level, spawn, enemies);
        self.start = start;
        self.player = Player::spawn(fx + 2, fy + 32 - Form::Small.height());
        self.player.set_form(player.form);
        self.player.facing_left = player.facing_left;
        self.status.start_clock(0);
        self.house = toad_house::ToadHouse::new(enemies, message, choice);
        self.map_house = false;
        if self.house.is_some() {
            self.freeze = toad_house::ARRIVAL_FREEZE;
        }
        self.level_generation = self.level_generation.wrapping_add(1);
    }

    /// Top-left of a GBA-sized view following the player, clamped to the level (the view
    /// `sma4-bevy` centres on the player).
    pub fn camera(&self) -> (i32, i32) {
        let cx = self.player.centre().0;
        let (w, h) = (SCREEN_W as i32, SCREEN_H as i32);
        if let Some(i) = &self.intro {
            return (i.camera_x, airship::OBJECT_CAMERA_Y);
        }
        if let Some(a) = &self.autoscroll {
            let (x, y) = a.camera();
            return (x, y.clamp(0, (self.level.pixel_height() - h).max(0)));
        }
        let (min_x, max_x) = self.level.camera_x.unwrap_or((0, (self.level.pixel_width() - w).max(0)));
        let (min_x, max_x) = (self.camera_limits.0.unwrap_or(min_x), self.camera_limits.1.unwrap_or(max_x));
        let x = (cx - w / 2).clamp(min_x, max_x.max(min_x));
        let y = self.level.camera_y.unwrap_or(self.camera_zones.y);
        (x, y)
    }

    /// 0x0808CAEC with the star: every projectile touching the player becomes a puff.
    fn puff_projectiles(&mut self) {
        use fortress::{Puff, PuffSlot};
        let player = &self.player;
        let puffs = &mut self.puffs;
        self.fireballs.retain(|f| {
            let hit = f.touches(player);
            if hit {
                puffs.push(Puff::from_projectile(f.px(), PuffSlot::Fireball));
            }
            !hit
        });
        self.boomerangs.retain(|b| {
            let hit = b.touches(player);
            if hit {
                puffs.push(Puff::from_projectile(b.px(), PuffSlot::Slot(b.slot)));
            }
            !hit
        });
        self.hammers.retain(|h| {
            let hit = h.touches(player);
            if hit {
                puffs.push(Puff::from_projectile(h.px(), PuffSlot::Slot(h.slot)));
            }
            !hit
        });
    }

    /// Projectile slots held by puffs: all of them, those of fire plants' fireballs, and
    /// the numbered ones.
    fn puff_slots(&self) -> (usize, usize, Vec<u8>) {
        use fortress::PuffSlot;
        let held = self.puffs.iter().filter(|p| p.slot != PuffSlot::None).count();
        let fire = self.puffs.iter().filter(|p| p.slot == PuffSlot::Fireball).count();
        let numbered = self.puffs.iter().filter_map(|p| if let PuffSlot::Slot(s) = p.slot { Some(s) } else { None }).collect();
        (held, fire, numbered)
    }

    /// The player takes a hit: big shrinks to small, small dies.
    pub fn hurt(&mut self) {
        // 0x0804A5F4 ignores hits while the star (+0x3D) runs.
        if self.invulnerable > 0 || self.dying.is_some() || self.player.transforming() || self.player.star != 0 {
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

    /// The player's splash and bubbles (0x0809F4C8 on going in or out, 0x08089B4C while
    /// in), then the effects' own update (0x0808A5B8, 0x08089E9C).
    fn step_water(&mut self, held: Buttons) {
        if let Some(change) = self.player.splash {
            self.splashes.retain(|s| s.slot != 0);
            self.splashes.push(water::Splash::player(&self.player, &self.level));
            if self.player.vy >= 0 && change == player::WaterChange::In {
                let rng = &self.rng;
                water::dive(&mut self.bubbles, &self.player, |i| rng.byte(i));
            }
        }
        if self.player.in_water {
            let moving = held.contains(Buttons::A) || held.contains(Buttons::LEFT) || held.contains(Buttons::RIGHT);
            water::breathe(&mut self.bubbles, &self.player, moving, self.frame);
        }
        let frame = self.frame;
        self.splashes.retain_mut(|s| s.step(frame));
        let level = &self.level;
        self.bubbles.retain_mut(|b| b.step(level));
    }

    /// Spawns the goal card when its column comes up (`enemies::Spawner`) and runs it; touching it starts the
    /// course clear (0x080684D4).
    fn step_goal(&mut self) {
        if let Some((x, y)) = self.goal_spawn
            && self.spawner.take_goal()
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

    /// A mini-fortress list entry whose column came up.
    fn spawn_fortress(&mut self, s: &sma4_assets::EnemySpawn) {
        match s.kind {
            fortress::PODOBOO => self.podoboos.push(fortress::Podoboo::spawn(s)),
            fortress::ROTO_DISC | fortress::ROTO_DISC_CCW => self.discs.push(fortress::RotoDisc::spawn(s)),
            boom_boom::BOOM_BOOM => {
                self.boom_boom = Some(boom_boom::BoomBoom::spawn(s.col as i32 * level::TILE_PX, s.row as i32 * level::TILE_PX))
            }
            larry::LARRY => self.larry = Some(larry::Larry::spawn(s.col as i32 * level::TILE_PX, s.row as i32 * level::TILE_PX)),
            fortress::CAMERA_MAX | fortress::CAMERA_MIN => {
                self.camera_limits = fortress::camera_limit(s, self.camera().0, self.camera_limits);
            }
            fortress::CHEST => self.chest = Some(fortress::Chest::spawn(s)),
            // 0x0804B768: the record's row byte.
            fortress::CHEST_ITEM => self.chest_item = s.row,
            fortress::CHEST_EXIT => self.chest_effect = true,
            _ => {}
        }
    }

    /// The fortress's objects for this frame; true if one hurt the player.
    fn step_fortress(&mut self) -> bool {
        let frame = self.frame;
        let mut hurt = false;
        let (px, py) = self.player.px();
        let pbox = (px, py, player::HITBOX_W, self.player.height());
        let star = self.player.star != 0;
        for (i, p) in self.podoboos.iter_mut().enumerate() {
            let ev = p.step(&self.level, frame, self.rng.byte(i));
            self.embers.extend(ev.ember.map(fortress::Ember::new));
            self.lava_splashes.extend(ev.splash.map(fortress::LavaSplash::new));
            if p.touches(pbox) {
                if star {
                    p.knock_out();
                    let (x, y) = p.px();
                    self.status.chain_popup(x, y, self.player.star_chain.min(STAR_CHAIN_MAX));
                    self.player.star_chain += 1;
                } else {
                    hurt = true;
                }
            }
        }
        let height = self.level.pixel_height();
        self.podoboos.retain(|p| p.px().1 < height + 32);
        for d in &mut self.discs {
            d.step();
            hurt |= d.touches(pbox) && !star;
        }
        self.embers.retain_mut(|e| e.step());
        self.lava_splashes.retain_mut(|s| s.step());
        self.sparks.retain_mut(|s| s.step(frame));
        self.puffs.retain_mut(|p| p.step());

        let camera_x = self.camera().0;
        if let Some(b) = &mut self.boom_boom {
            let (bx, by) = b.px();
            let touching = b.touches(pbox);
            // 0x08048F80 with +0x40 set: a landing when the frame is at least 8 px above him.
            let stomp = touching && self.player.vy >= 0 && self.player.frame().1 <= by - 8;
            if star && touching && let Some(ball) = b.star_hit() {
                self.status.chain_popup(bx, by, self.player.star_chain.min(STAR_CHAIN_MAX));
                self.player.star_chain += 1;
                self.sparks.extend(fortress::Spark::burst(b.x, b.y));
                self.ball = Some(ball);
                self.boom_boom = None;
                self.clock_stopped = true;
                return hurt;
            }
            let ev = b.step(&self.level, &self.player, camera_x, self.player.on_ground, stomp, frame);
            if let Some(c) = ev.lock_camera {
                self.camera_limits = (Some(c), Some(c));
                // 0x0806CFEC: scroll mode 2's window from where the camera is.
                self.camera_zones.rebase();
                self.player.vx = 0;
                if b.phase == 1 {
                    self.boss_freeze = true;
                }
            }
            if let Some((tx, ty)) = ev.block {
                self.level.set(tx, ty, boom_boom::INTRO_BLOCK);
            }
            self.puffs.extend(ev.puff.map(fortress::Puff::new));
            if ev.camera_step {
                self.camera_limits = (self.camera_limits.0.map(|x| x + 1), self.camera_limits.1.map(|x| x + 1));
            }
            if ev.fight {
                self.boss_freeze = false;
            }
            if let Some(kind) = ev.stomped {
                self.player.vy = boom_boom::STOMP_BOUNCE;
                self.player.on_ground = false;
                self.status.popup(bx, by, kind);
            }
            hurt |= ev.hurt_player || (touching && !stomp && !star);
            if let Some(ball) = ev.burst {
                self.clock_stopped = true;
                self.sparks.extend(fortress::Spark::burst(b.x, b.y));
                self.ball = Some(ball);
                self.boom_boom = None;
            }
        }
        // Player fireballs: five beat him (+0x36).
        if let Some(b) = &mut self.boom_boom {
            for f in &mut self.player_fireballs {
                let (fx, fy) = f.px();
                if f.live() && b.touches((fx, fy, 8, 8)) {
                    b.fireball();
                    f.hit();
                }
            }
        }
        if let Some(ball) = &mut self.ball
            && ball.step(&self.level, pbox)
        {
            // 0x0806DDD4: course clear music 0x21, the clock stops and is counted.
            self.fortress_clear = Some(goal::ClockTally::new(boom_boom::BALL_TALLY_DELAY, boom_boom::BALL_TALLY_POINTS));
            self.ball = None;
        }
        // 0x0804D12C: no projectile and no Bro left, a chest drops in on the far side
        // (camera + 0xC0, or + 0x30 with the player right of 0x80).
        if self.chest_effect
            && self.chest.is_none()
            && self.exit_timer == 0
            && self.fireballs.is_empty()
            && self.micro_goombas.is_empty()
            && self.boomerangs.is_empty()
            && self.hammers.is_empty()
            && self.puffs.iter().all(|p| p.slot == fortress::PuffSlot::None)
            && !self.enemies.iter().any(|e| enemies::is_bro(e.kind))
        {
            let x = self.camera().0 + if self.player.x < fortress::CHEST_SIDE * player::SUB { 0xC0 } else { 0x30 };
            self.chest = Some(fortress::Chest::at(x, fortress::CHEST_DROP_Y));
        }
        if let Some(c) = &mut self.chest {
            let item = self.battle.unwrap_or(self.chest_item);
            let ev = c.step(&self.level, item, pbox);
            self.puffs.extend(ev.puff.map(fortress::Puff::new));
            self.clock_stopped |= ev.opened;
            if let Some(item) = ev.got {
                self.inventory.push(item);
                self.chest = None;
                self.exit_timer = fortress::CHEST_EXIT_FRAMES;
            }
        }
        if let Some(c) = &mut self.crusher {
            c.step(frame);
            self.level.shift = Some((c.split_y(), c.shift()));
            self.level.camera_y = Some(c.base());
            // 0x08092030: the head against the spikes.
            let (fx, fy) = self.player.frame();
            let head = if self.player.form == Form::Small { crusher::HEAD_SMALL } else { crusher::HEAD_BIG };
            let y = fy + head - c.shift();
            let id = self.level.collision_id((fx + 8).div_euclid(level::TILE_PX), y.div_euclid(level::TILE_PX));
            if id.is_some_and(|id| id == crusher::HURT_TILE || crusher::SPIKES.wrapping_sub(id) < 2) {
                hurt = true;
            }
        }
        hurt
    }

    /// The end of a fortress: the ball's tally then the map (cleared), or the chest's
    /// countdown then the map (not cleared). The player keeps moving meanwhile. True once
    /// the level is over.
    fn step_fortress_end(&mut self) -> bool {
        if self.exit_timer != 0 {
            self.exit_timer -= 1;
            if self.exit_timer == 0 {
                // A battle won: its map object is gone.
                if self.battle.take().is_some()
                    && let Some(map) = &mut self.world_map
                {
                    map.battle_won();
                }
                self.to_map(world_map::Arrival::Walk);
                return true;
            }
            return false;
        }
        let Some(t) = &mut self.fortress_clear else { return false };
        let (points, done) = t.step(&mut self.status.clock);
        self.status.add_score(points);
        if done {
            self.fortress_clear = None;
            self.finish_level();
        }
        done
    }

    /// Coins and debris from a block hit.
    fn block_outcome(&mut self, tx: i32, ty: i32, outcome: HitOutcome) {
        let coin = match outcome {
            HitOutcome::Coin => true,
            HitOutcome::Broken { coin } => {
                // 0x080A0024: 10 points (0x030038AC = 1) and the pieces.
                self.status.add_score(1);
                self.debris.push(blocks::BrickDebris::new(tx, ty));
                coin
            }
            HitOutcome::Item | HitOutcome::Nothing => false,
        };
        if coin {
            self.status.add_coin();
            self.level_coins = self.level_coins.wrapping_add(1);
            self.coin_pops.push(CoinPop::new(tx, ty));
        }
    }

    /// One frame of the course clear: the player walks off, coins pay out, the goal runs
    /// its texts and clock tally; then back to the map.
    fn step_course_clear(&mut self, camera: (i32, i32)) {
        let stop = Goal::player_stopped(&self.player, camera);
        self.player.goal_walk(&self.level, self.frame, stop);
        self.skid_dust.step(&self.player);
        let (level, frame) = (&mut self.level, self.frame);
        let player = &mut self.player;
        self.bumps.retain_mut(|b| !b.step(level, player, false).1);
        let (level, player) = (&*level, &self.player);
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

    /// 0x08092214: at the screen's edges (frame screen x < 5 or > 0xDF) the player moves at
    /// least (left edge) or at most (right edge) as fast as the scroll, `speed`.
    fn push_along(&mut self, speed: i32) {
        let sx = self.player.frame().0 - self.camera().0;
        let p = &mut self.player;
        let push = (sx < autoscroll::EDGE_LEFT && p.vx < speed) || (sx > autoscroll::EDGE_RIGHT && p.vx > speed);
        // TODO(re): 0x08092214 also clears byte +0 of the player struct when the speeds
        // have the same sign; done to this port's x fraction it would stop the push.
        if push {
            p.vx = speed;
        }
        // TODO(re): the ROM's clamp at the screen's left edge is not located; friction
        // between pushes would otherwise let the player drift off it.
        if sx < 0 {
            p.x -= sx * player::SUB;
        }
    }

    /// The Toad House's frame; true once the visit is over (back on the map).
    fn step_house(&mut self, pressed: Buttons, frozen: bool) -> bool {
        let camera_y = self.camera().1;
        let Some(h) = &mut self.house else { return false };
        let ev = h.step(&mut self.player, &self.level, pressed, frozen, camera_y, self.frame, self.rng.byte(0));
        if let Some(item) = ev.got {
            self.inventory.push(item);
        }
        // The item (+0x40 = 2 every frame) and the exit (0x08007594) hold the player.
        if h.busy() {
            self.freeze = self.freeze.max(toad_house::ITEM_FREEZE);
        }
        if ev.leave {
            // A map house's spot is done (mGBA: tile 0x40 after the visit); the hidden one
            // leaves 1-3 not cleared, Mario back on its spot.
            self.house = None;
            if std::mem::take(&mut self.map_house)
                && let Some(map) = &mut self.world_map
            {
                map.clear_here();
            }
            self.to_map(world_map::Arrival::Walk);
            return true;
        }
        false
    }

    /// The throne room's frame (0x08093CBC from the object loop); true once Mario leaves.
    fn step_throne(&mut self) -> bool {
        let (Some(t), Some(d)) = (&mut self.throne, &self.castle_data) else { return false };
        let leave = t.step(d, self.input.pressed);
        self.obj_sprites = t.sprites();
        match leave {
            // 0x08094140: the map's slot 1 becomes the airship (kind 2), slot 0 empty, and
            // the area switch 3 to the room's junction (the airship's intro, level 0x202).
            Some(castle::Leave::Airship) => {
                if let Some(map) = &mut self.world_map {
                    map.objects.slots[1].kind = castle::MAP_AIRSHIP;
                    map.objects.slots[0].kind = 0;
                }
                self.throne = None;
                self.obj_sprites.clear();
                self.curtain_to = pipes::destination(&self.level, &self.player);
                self.curtain = Some(castle::Curtain::new(castle::CurtainKind::MarioStart));
                true
            }
            // 0x03003851 = 0, sub-state 0x1B: back to the map.
            Some(castle::Leave::Map) => {
                self.throne = None;
                self.obj_sprites.clear();
                self.to_map(world_map::Arrival::Walk);
                true
            }
            None => false,
        }
    }

    /// Between the castle's rooms: the level is held while the curtain runs; the next room
    /// is asked for on its load frame. True while it holds the game.
    fn step_curtain(&mut self) -> bool {
        let Some(c) = &mut self.curtain else { return false };
        let ev = c.step();
        self.obj_sprites = c.sprites();
        if ev.load {
            self.pending_area = self.curtain_to.take();
            self.pipe = self.pending_area.as_ref().map(|_| Pipe::Switching);
        }
        if !ev.done {
            return true;
        }
        self.curtain = None;
        self.obj_sprites.clear();
        false
    }

    /// The shell in the player's hands (state 4, 0x08043F0C), after the objects' update:
    /// B let go kicks it (0x080436A0, with the kick pose); a shell that woke up in a wall
    /// scores 100 (0x08093560 kind 5); touching another object knocks both out
    /// (0x08043434 -> 0x08048868, 100 points, chain 0). The player's flag 0x03007820 + 0
    /// follows.
    fn step_carried(&mut self, b_held: bool) {
        for e in &mut self.enemies {
            if e.woke_stuck {
                let (x, y) = e.px();
                self.status.popup(x, y, 5);
            }
            if matches!(e.state, EnemyState::Carried(_)) && !b_held {
                e.release(&self.player);
                self.player.kick = player::KICK_POSE_FRAMES;
            }
        }
        if let Some((x, y)) = enemies::carried_hits(&mut self.enemies, &self.player) {
            self.status.chain_popup(x, y, 0);
        }
        self.player.carrying = self.enemies.iter().any(|e| matches!(e.state, EnemyState::Carried(_)));
    }

    /// The airship's deck (`ship`), after the objects as in 0x0808C4EC: smoke, then the
    /// cannonballs (a stomp bounces the player and scores the chain, 0x0808E05C), then the
    /// generators. Returns whether a ball hurt the player.
    fn step_ship(&mut self, hidden: bool) -> bool {
        if self.ship == ship::Ship::default() {
            return false;
        }
        let view = ship::View { camera: self.camera(), scroll_dy: self.scroll_step.1, counter: self.frame as u8 };
        let autoscroll = self.autoscroll.is_some();
        self.ship.step_smoke(&view, self.scroll_step.0, autoscroll);
        let camera_y = self.camera().1;
        let hits = self.ship.step_balls(&view, autoscroll, &self.player, camera_y);
        for (x, y) in &hits.stomped {
            self.player.vy = ship::BALL_BOUNCE;
            self.player.on_ground = false;
            // TODO(re): 0x0808C8E0 -> 0x0808C850(5, 0x0809347C) taken as the stomp chain.
            self.status.chain_popup(*x, *y, self.player.stomp_chain);
            self.player.stomp_chain += 1;
        }
        self.ship.step_propellers(view.camera.0);
        self.obj_sprites = self.ship.sprites(&view, &self.enemies);
        let rand = self.rng.bytes();
        self.ship.step_guns(&view, &rand, &self.player, &mut self.enemies);
        hits.hurt && !hidden
    }

    /// Drops Larry's wand as his state 1 does once he has flown off the top (0x0807230C);
    /// debug starts (`--wand`) call this in his cabin too.
    pub fn drop_wand(&mut self) {
        self.wand = Some(wand::Wand::spawn());
    }

    /// Larry Koopa (`larry`), with the objects: his main, then his rings (with the
    /// projectiles), then the player's fireballs on him (0x0808B2F0). His and the rings'
    /// OAM entries go behind the player. True if he or a ring hurt the player.
    fn step_larry(&mut self, hidden: bool) -> bool {
        let camera = self.camera();
        let Some(l) = &mut self.larry else { return false };
        let counter = self.frame as u8;
        let mut view = larry::PlayerView::of(&self.player);
        view.ignore = hidden;
        // 0x03007848 + 0xBD1: everything waits while the player changes form.
        let held = self.player.transforming();
        let ev = l.step(&self.level, &view, camera, counter, held);
        let mut hurt = ev.hurt_player;
        if ev.bounce {
            self.player.vy = larry::STOMP_BOUNCE;
            self.player.on_ground = false;
        }
        if let Some(vx) = ev.push {
            self.player.vx = vx;
            // 0x080735DC: +0x70 = 8 (`Player::push_timer`).
            self.player.push_timer = larry::PUSH_TIME;
        }
        // 0x03003D0F = 0x81: the clock stops; objects in slots 4..0 become puffs.
        if ev.beaten {
            self.clock_stopped = true;
            self.enemies.iter_mut().for_each(enemies::Enemy::larry_beaten);
        }
        for f in &mut self.player_fireballs {
            if f.live() && l.fireball(f.px()) {
                f.hit();
            }
        }
        // 0x0808CC9C -> 0x0808CAEC: a ring hurts; with the star it becomes a puff.
        if !hidden {
            if self.player.star != 0 {
                for at in l.take_rings_touching(&self.player) {
                    self.puffs.push(fortress::Puff::from_projectile(at, fortress::PuffSlot::None));
                }
            } else if l.ring_touches(&self.player) {
                hurt = true;
            }
        }
        self.obj_sprites = l.sprites(camera, counter);
        self.obj_front = 0;
        if ev.wand {
            self.drop_wand();
        }
        hurt
    }

    /// The wand with the projectiles (0x0808FBD0, after the objects): the player's fireballs
    /// and tail bump it (0x0808E9B0 / 0x0808EAA4), it sparkles, falls, is drawn, and the
    /// draw tests the player (0x0808CC9C -> 0x0808CAEC). Its sparkles and smoke are effects,
    /// which run before the projectiles; the tail's hit star (0x0808C114) and a statue's
    /// burst (0x08088EF4) after them (0x0808C4EC).
    fn step_wand(&mut self) {
        let camera = self.camera();
        let effects = self.step_wand_sparkles(camera);
        self.step_wand_projectile(camera, effects);
        let mut star = Vec::new();
        if let Some(h) = &mut self.hit_star
            && !h.step(camera, self.frame, &mut star)
        {
            self.hit_star = None;
        }
        // OAM 0 and 1: in front of everything.
        self.obj_front += star.len();
        self.obj_sprites.splice(0..0, star);
        let mut burst = Vec::new();
        if let Some(b) = &mut self.statue_burst
            && !b.step(camera, &mut burst)
        {
            self.statue_burst = None;
        }
        self.obj_sprites.extend(burst);
    }

    /// 0x0808ED40 for the wand; `effects` are the effects' OAM entries this frame.
    fn step_wand_projectile(&mut self, camera: (i32, i32), effects: Vec<obj::ObjSprite>) {
        let Some(w) = &mut self.wand else {
            self.obj_sprites.extend(effects);
            return;
        };
        // 0x0808FB08 before the handler.
        w.count_down();
        if w.wait == 0 {
            let seen: Vec<(usize, (i32, i32))> = (self.player_fireballs.iter().enumerate())
                .filter_map(|(i, f)| f.seen_by_projectiles().map(|at| (i, at)))
                .collect();
            if let Some(k) = w.fireball_hit(seen.iter().map(|&(_, at)| at)) {
                // Sound 0x60: the fireball bursts (type 3).
                self.player_fireballs[seen[k].0].burst_by_projectile();
            }
            if w.tail_hit(&self.player) {
                self.hit_star = Some(wand::HitStar::at_tail(&self.player));
            }
        }
        self.wand_sparkles.extend(w.sparkle(camera, self.frame));
        w.statue_bounce(&mut self.player);
        w.step(&self.level);
        // Larry's step rebuilt `obj_sprites` this frame; without him (`--wand`) they are
        // the wand's alone.
        if w.gone(camera) {
            self.wand = None;
            if self.larry.is_none() {
                self.obj_sprites.clear();
                self.obj_front = 0;
            }
            self.obj_sprites.extend(effects);
            return;
        }
        let sprites = w.sprites(camera);
        self.obj_front = sprites.len();
        if self.larry.is_some() {
            self.obj_sprites.splice(0..0, sprites);
        } else {
            self.obj_sprites = sprites;
        }
        // The effects' OAM entries come after the player's (index 0x10 on).
        self.obj_sprites.extend(effects);
        // 0x0808CAEC: not while the post-hurt flash runs (+0x3C) or the player is dying.
        if self.invulnerable == 0 && self.dying.is_none() && w.touches(&self.player) {
            if self.player.statue != 0 {
                // A Tanooki statue (+0x52): it bounces off once it turns back.
                let (burst, smoke) = w.statue_touch(&mut self.player, camera);
                self.statue_burst = Some(burst);
                self.wand_smoke = Some(smoke);
                return;
            }
            // The catch: the clock stops (0x03003BB0 / 0x03003771) and is kept for the
            // tally line, the slot is freed (its OAM entries stay for this frame).
            self.wand = None;
            self.wand_clear = Some(wand::Cleared::new(self.status.clock));
            self.clock_stopped = true;
            // 0x0808CA90: the objects still around become puffs.
            for e in &mut self.enemies {
                e.wand_caught();
            }
        }
    }

    /// The wand's sparkles (0x08091F6C -> 0x08091420): one frame each, their OAM entries.
    fn step_wand_sparkles(&mut self, camera: (i32, i32)) -> Vec<obj::ObjSprite> {
        let mut out = Vec::new();
        self.wand_sparkles.retain_mut(|s| s.step(camera, false, &mut out));
        // A statue's smoke (effect 1, 0x0809054C; the camera does not move in the cabin).
        if let Some(s) = &mut self.wand_smoke {
            if s.life & 0x7F == 0 {
                self.wand_smoke = None;
            } else {
                s.life -= 1;
                s.sprites(self.frame as u8, &mut out);
            }
        }
        out
    }

    /// Larry's states 3-7 after the catch (`wand::Cleared`). While the state is 3 or more
    /// the player is held (0x08094BD8) and drawn in pose 0x0813DEE8[form] (the jump pose)
    /// with the wand over his head (0x080A83EC); from state 7 he falls 2 px a frame
    /// (0x080A8000) through the sky, the clouds rising past him. Everything else waits.
    fn step_wand_clear(&mut self) -> bool {
        let Some(c) = &mut self.wand_clear else { return false };
        // The player's update comes before the objects.
        if c.falling() {
            self.player.y += wand::FALL_STEP * player::SUB;
        }
        // The clouds are drawn (0x08071EC4) before they move.
        let clouds = c.cloud_sprites();
        let ev = c.step(&mut self.status.clock, self.frame, self.player.frame().1);
        self.status.add_score(ev.points);
        self.level_hidden = c.level_hidden;
        self.backdrop = (c.state >= 5).then_some(c.backdrop);
        // 0x08073210: speed 0.
        self.player.vx = 0;
        self.player.forced_pose = Some(player::Pose::Jump);
        let camera = self.camera();
        let (fx, fy) = self.player.frame();
        self.obj_sprites = wand::held_sprites(&self.player, (fx - camera.0, fy - camera.1), self.frame);
        // The wand's entries come before the player's, the clouds' after.
        self.obj_front = self.obj_sprites.len();
        self.obj_sprites.extend(clouds);
        let sparkles = self.step_wand_sparkles(camera);
        self.obj_sprites.extend(sparkles);
        // The object loop runs on: the puffs 0x0808CA90 left fade out.
        let (level, player, frame) = (&self.level, &self.player, self.frame);
        self.enemies.retain_mut(|e| e.step(level, player, frame));
        if ev.done {
            // TODO(port): what follows (mode 1 sub-states 0x1B..: the fade, the King changed
            // back, the letter, "WORLD 1 CLEAR!") is not ported; the level ends as cleared.
            self.wand_clear = None;
            self.obj_sprites.clear();
            self.obj_front = 0;
            self.level_hidden = false;
            self.backdrop = None;
            self.player.forced_pose = None;
            self.finish_level();
        }
        true
    }

    /// The airship's intro (`airship::Intro`): the player is scripted, the clock holds
    /// (the player is busy, 0x08094BD8). True while it holds the game.
    fn step_intro(&mut self) -> bool {
        let Some(intro) = &mut self.intro else { return false };
        let ev = intro.step(&mut self.player, self.frame);
        self.obj_sprites = intro.sprites();
        self.level.shift = Some((self.level.pixel_height(), intro.level_shift()));
        self.bg_scroll = Some((intro.camera_x >> 2, airship::INTRO_BG_Y));
        if ev.switch {
            self.curtain_to = pipes::destination(&self.level, &self.player);
            self.curtain = Some(castle::Curtain::new(castle::CurtainKind::Plain));
        }
        true
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
        self.to_map(world_map::Arrival::Walk);
    }

    /// After a death (0x080270B8): one life less, back to the map small; with no life
    /// left (lives & 0x8000), the game-over screen.
    fn lose_life(&mut self) {
        self.dying = None;
        self.battle = None;
        self.player.set_form(Form::Small);
        // Lives go below zero (0xFFFF: the bars show 0) on game over.
        self.status.lives = self.status.lives.wrapping_sub(1);
        if self.status.lives == status::NO_LIVES {
            if let Some(map) = &mut self.world_map {
                map.game_over();
            }
            // The game-over menu (0x08012458) ends the spade streak.
            self.spade_streak = spade::Streak::default();
            self.screen = Screen::GameOver(GameOver::new());
        } else {
            self.to_map(world_map::Arrival::Panel);
        }
    }

    /// QUIT's fade is over: the title screen (sub-state 9).
    pub fn show_title(&mut self) {
        self.title = self.title_data.clone().map(title::Title::new);
        self.screen = Screen::Title;
    }

    /// A new game from the title (0x0802DF94 -> sub-states 0x0A, 0x0F: a new file): World
    /// 1's map with its "WORLD 1" panel, 4 lives, nothing cleared or carried.
    fn new_game(&mut self) {
        self.status = Status::default();
        self.spade_streak = spade::Streak::default();
        self.inventory.clear();
        self.player.set_form(Form::Small);
        if let Some(map) = &mut self.world_map {
            map.reset();
        }
        self.screen = Screen::Map;
    }

    /// Enters the level again from the map: the level as it was first entered, a fresh
    /// clock, the player at the start in its current form.
    pub fn restart_level(&mut self) {
        if let Some((level, enemies)) = self.start.clone() {
            let form = self.player.form;
            self.enter_level(level, self.spawn, &enemies);
            self.player.set_form(form);
            self.player.start(self.level.start_kind, &self.level);
        // A Starman used on the map (0x08006454: player +0x5C).
        if std::mem::take(&mut self.pending_star) {
            self.player.starman();
        }
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
        self.player.start(self.level.start_kind, &self.level);
        self.status.popups.clear();
        self.set_time_limit(time_limit);
        self.level_generation = self.level_generation.wrapping_add(1);
        self.screen = Screen::Level;
    }

    /// A spade spot (tileset 15, game kind 0x030038EC = enemy word & 0x7FFF) starts its
    /// game (0x08009790); false for anything else.
    pub fn enter_spade(&mut self, node: &sma4_assets::MapNode) -> bool {
        let slots = node.tileset == spade::TILESET && node.enemies & 0x7FFF == spade::KIND_SLOTS;
        let Some(data) = self.spade_data.as_ref().filter(|_| slots) else { return false };
        self.spade = Some(spade::Spade::new(data.clone(), self.spade_streak));
        self.screen = Screen::Spade;
        true
    }

    /// Back to the map.
    fn to_map(&mut self, arrival: world_map::Arrival) {
        if let Some(map) = &mut self.world_map {
            map.show(arrival);
        }
        self.screen = Screen::Map;
    }

    /// Starts the level clock (seconds; 0 = no clock).
    pub fn set_time_limit(&mut self, seconds: u16) {
        self.time_limit = seconds;
        self.status.start_clock(seconds);
        // The throne room hides and holds the clock (0x08093CE4: 0x03003D0F = 0x81); its
        // digits stay for the airship, whose landing clears 0x03003D0F (0x0809F43C).
        let throne = self.throne.is_some();
        self.status.clock_hidden = throne;
        self.clock_stopped |= throne;
    }

    /// One frame of the level's Advance Coins (0x08077720). The objects are frozen (+0xBD1)
    /// while one of them held the game last frame (+0xDF8) or the player is dying.
    fn step_advance_coins(&mut self) {
        let dying = self.dying.as_ref().map(|d| d.vy);
        let cx = advance_coin::Context {
            camera: self.camera(),
            coins: self.level_coins,
            quota: self.coin_quota,
            frozen: dying.is_some() || self.advance_coins.iter().any(|c| c.freezes()),
            dying,
        };
        for c in &mut self.advance_coins {
            c.step(&cx, &mut self.player);
        }
        self.advance_taken |= self.advance_coins.iter().any(|c| c.taken());
    }

    /// The item panel and the Warp Whistle on the map; true while they hold the map.
    fn step_map_items(&mut self) -> bool {
        let pressed = self.input.pressed;
        if let Some(w) = &mut self.warp {
            let ev = w.step(self.frame);
            if ev.switch_world {
                self.pending_world = Some(warp::WARP_ZONE);
                self.status.world = warp::WARP_ZONE + 1;
            }
            if ev.done {
                self.warp = None;
            }
            return true;
        }
        if let Some(panel) = &mut self.item_panel {
            let step = panel.step(pressed, &mut self.inventory);
            if step.done {
                self.item_panel = None;
                match step.used {
                    Some(inventory::Used::Form(item)) => self.player.set_form(match item {
                        inventory::MUSHROOM => Form::Big,
                        inventory::FIRE_FLOWER => Form::Fire,
                        _ => Form::Raccoon,
                    }),
                    Some(inventory::Used::Starman) => self.pending_star = true,
                    Some(inventory::Used::WarpWhistle) => {
                        if let Some(map) = &self.world_map {
                            self.warp = Some(warp::Warp::start(map.sprite_pos().0, map.y));
                        }
                    }
                    None => {}
                }
            }
            return true;
        }
        // 0x0800D5B4: L on the idle map opens the panel.
        let idle = self.world_map.as_ref().is_some_and(|m| m.phase == world_map::MapPhase::Idle && !m.walking());
        if idle && pressed.contains(Buttons::L) {
            self.item_panel = Some(inventory::Inventory::open());
            return true;
        }
        false
    }

    /// World 9 has been loaded after the Warp Whistle: Mario's spot there (0x08007950).
    pub fn enter_world(&mut self, mut map: world_map::WorldMap) {
        map.show(world_map::Arrival::Walk);
        (map.y, map.x) = warp::LANDING;
        self.world_map = Some(map);
    }

    /// Advances one frame.
    pub fn step(&mut self, held: Buttons) {
        self.input.pressed = held & !self.input.held;
        self.input.held = held;
        self.frame = self.frame.wrapping_add(1);
        // 0x08000698: the random bytes move on every frame.
        self.rng.step();
        match &mut self.screen {
            Screen::Map => {
                if self.step_map_items() {
                    return;
                }
                if let Some(map) = &mut self.world_map {
                    map.rand = self.rng.bytes();
                    map.frame = self.frame;
                    if let Some(node) = map.step(held, self.input.pressed) {
                        self.battle = map.meeting.map(|(_, item)| item);
                        if node.tileset == toad_house::TILESET && node.enemies & 0x8000 != 0 {
                            // 0x08009A3C: a bit-15 enemy word is the chest choice.
                            self.house_choice = Some((node.enemies >> 8) as u8 & 0x7F);
                            self.pending_house = Some(node.level);
                        } else if !self.enter_spade(&node) {
                            self.pending_level = Some(node);
                        }
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
                        self.to_map(world_map::Arrival::World);
                    }
                    // The fade, then the title screen.
                    Some(MenuChoice::Quit) => {}
                    Some(MenuChoice::Quitted) => self.show_title(),
                    None => {}
                }
                return;
            }
            Screen::Title => {
                let pressed = self.input.pressed;
                match self.title.as_mut().map(|t| t.step(pressed)) {
                    // Without the title's assets: a new game straight away.
                    None => self.new_game(),
                    Some(Some(title::TitleEvent::Seed(b))) => self.rng.set_first(b),
                    Some(Some(title::TitleEvent::Start { .. })) => {
                        self.title = None;
                        self.new_game();
                    }
                    Some(None) => {}
                }
                return;
            }
            Screen::Spade => {
                let pressed = self.input.pressed;
                let done = match &mut self.spade {
                    Some(s) => s.step(pressed, &self.rng, self.frame, &mut self.spade_streak, &mut self.status.lives),
                    None => true,
                };
                if done {
                    // State 3 (0x080343FC) leaves like a cleared level (sub-state 0x1C).
                    self.spade = None;
                    if let Some(map) = &mut self.world_map {
                        map.clear_here();
                    }
                    self.to_map(world_map::Arrival::Walk);
                }
                return;
            }
            Screen::Level => {}
        }
        if self.level.width == 0 {
            return;
        }
        if self.step_curtain() {
            return;
        }
        if self.step_intro() {
            return;
        }
        if self.step_wand_clear() {
            return;
        }
        if let Some(d) = &mut self.dying {
            match d.step() {
                Some(dy) => self.player.y += dy,
                None => {
                    self.lose_life();
                    return;
                }
            }
            // A collected Advance Coin falls out of the corner (0x0807742C).
            self.step_advance_coins();
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
            // 0x080A8764 / 0x080A8CC0: the end of a way out down or sideways sets scroll
            // mode 2's window from the camera (the way out up, 0x080A86F4, does not).
            if matches!(pipe, Pipe::Exit { dir, .. } if dir != pipes::UP) && self.pipe.is_none() {
                self.camera_zones.rebase();
            }
            if pipe != Pipe::Switching && self.pipe == Some(Pipe::Switching) {
                let world = (self.status.world as usize).saturating_sub(1);
                self.pending_area = if self.pipe_to_end {
                    self.end_rooms.and_then(|r| r.get(world).map(pipes::end_room))
                } else {
                    pipes::destination(&self.level, &self.player)
                };
                if self.pending_area.is_none() {
                    self.pipe = None;
                }
            }
            return;
        }

        // Advance Coins (docs/re-notes.md): while one flies to the corner (+0xDF8) the whole
        // game waits. The player touches an idle one through its contact (`step_advance_coins`)
        // only, so its solid cell is left out of his own tile collision.
        self.level.solid_cells.clear();
        if self.advance_coins.iter().any(|c| c.freezes()) {
            self.step_advance_coins();
            return;
        }
        // The throne room holds the player every frame (+0x40 = 2, 0x08093E18).
        if self.throne.is_some() {
            self.freeze = self.freeze.max(1);
        }
        // Frozen (+0x40): no input, no movement (0x080A8000 only draws the player).
        let frozen = self.freeze > 0 || self.boss_freeze;
        self.freeze = self.freeze.saturating_sub(1);
        if self.step_fortress_end() {
            return;
        }
        let (held, pressed) = if frozen { (Buttons::empty(), Buttons::empty()) } else { (held, self.input.pressed) };
        self.player.camera_y = self.camera().1;
        // 0x080A7D94 (+0x45 = 4): only once the autoscroll runs; up with gravity 4 and along
        // with the scroll until the speed is no longer negative, then the usual control.
        let landing = self.landing.is_some();
        if let Some(vy) = self.landing
            && self.autoscroll.is_some()
        {
            self.player.y += vy as i32 * 16;
            self.landing = (vy < 0).then_some(vy.wrapping_add(4));
            self.player.vy = vy.wrapping_add(4) as i32 * player::UNIT;
            if self.landing.is_none() {
                self.player.forced_pose = None;
                // 0x080A7D94: +0x45 = 0 and 0x03003D0F = 0, the clock shows and runs.
                self.status.clock_hidden = false;
                self.clock_stopped = false;
            }
        }
        let was_left = self.player.facing_left;
        let head_hit = if frozen || landing { None } else { self.player.step(held, pressed, &self.level, self.frame) };
        self.player.step_hold(was_left);
        // Tile 0x79 on an idle Advance Coin's cell (0x080750F0) for everything else.
        let camera = self.camera();
        self.level.solid_cells = self.advance_coins.iter().filter(|c| c.solid(camera)).map(|c| c.cell()).collect();
        self.skid_dust.step(&self.player);
        self.step_water(held);
        self.camera_zones.step(&self.player);
        if let Some(t) = self.warp_trigger
            && t.hit(&self.player)
        {
            self.warp_trigger = None;
            self.pending_house = Some(toad_house::HIDDEN_HOUSE);
            return;
        }
        if self.step_house(pressed, frozen) {
            return;
        }
        if self.step_throne() {
            return;
        }
        // 0x080A8000: flying out of the top of the area (+0x27 sky flight, frame y < 0):
        // the area switch 0x03003CA0 = 3, by the pointer for the page the player is on.
        if self.player.sky < 0
            && self.player.frame().1 < 0
            && let Some(to) = pipes::destination(&self.level, &self.player)
        {
            self.pending_area = Some(to);
            self.pipe = Some(Pipe::Switching);
            return;
        }
        // 0x0808C068 -> 0x0808BFEC: a free slot (a bursting fireball still holds its own).
        if self.player.wants_fireball(self.input.pressed) && self.player_fireballs.len() < fire::FIREBALL_SLOTS {
            self.player_fireballs.push(fire::PlayerFireball::thrown(&self.player));
            self.player.throw = player::THROW_TIME;
        }
        // Doors (0x0809588C).
        if !self.player.transforming()
            && self.crusher.is_none_or(|c| c.door_open())
            && let Some(snap) = doors::entry(&self.level, &self.player, pressed, self.level.class)
        {
            if let Some(x) = snap {
                let fx = self.player.frame().0;
                self.player.x += (x - fx) * player::SUB;
            }
            self.pending_area = pipes::destination(&self.level, &self.player);
            if self.pending_area.is_some() {
                self.pipe = Some(Pipe::Switching);
                self.player.vx = 0;
                return;
            }
        }
        if !self.player.transforming()
            && let Some((dir, to_end)) = pipes::entry(&self.level, &self.player, held)
        {
            self.pipe = Some(Pipe::enter(dir));
            self.pipe_to_end = to_end;
            return;
        }
        if self.player.p_meter & 0x40 != 0 {
            self.p_blink = self.p_blink.wrapping_sub(1);
        }
        // Blocks react the same to a head hit and to the raccoon's tail (0x0809A748); the
        // feet probe bumps note blocks the player stands on.
        let hits = head_hit
            .map(|t| (t, HitFrom::Below, true))
            .into_iter()
            .chain(self.player.tail_block().map(|t| (t, HitFrom::Side, false)))
            .chain(self.player.feet_block(&self.level).map(|t| (t, HitFrom::Above, false)));
        for ((tx, ty), from, head) in hits.collect::<Vec<_>>() {
            // An Advance Coin's cell is no block (the coin handles the hit itself).
            if self.level.solid_cells.contains(&(tx, ty)) {
                continue;
            }
            if self.bumps.len() < MAX_BUMPS
                && let Some((bump, outcome)) = blocks::hit_block(&mut self.level, &self.player, tx, ty, from)
            {
                self.block_outcome(tx, ty, outcome);
                if let Some(bump) = bump {
                    if head {
                        self.player.vy = blocks::HEAD_BUMP_PUSH;
                    }
                    self.bumps.push(bump);
                }
            }
        }
        // Level coins: one coin and 50 points each (0x08094780).
        for (tx, ty) in coins::collect(&mut self.level, &self.player, self.frame) {
            self.status.add_coin();
            self.level_coins = self.level_coins.wrapping_add(1);
            self.status.add_score(5);
            self.sparkles.push(Sparkle::new(tx, ty));
        }

        let view = self.camera();
        let (level, player) = (&mut self.level, &mut self.player);
        let a_pressed = self.input.pressed.contains(Buttons::A);
        let mut spawned = Vec::new();
        self.bumps.retain_mut(|b| {
            let (item, done) = b.step(level, player, a_pressed);
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
        self.debris.retain_mut(|d| d.step(frame, view));
        // 0x08094780: the multi-coin block's timer.
        self.level.coin_timer = self.level.coin_timer.saturating_sub(1);

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
            match kind {
                blocks::LEAF => self.player.leaf(),
                blocks::FIRE_FLOWER => self.player.fire_flower(),
                blocks::STAR => self.player.starman(),
                _ => self.player.grow(),
            }
            self.status.popup(x, y, 9);
        }

        self.invulnerable = self.invulnerable.saturating_sub(1);
        let before = self.enemies.len();
        // Autoscroll (0x08092BF0, after the player and the objects): held while the player
        // is busy (transforming).
        if !self.player.transforming()
            && let (Some(a), Some(scripts)) = (&mut self.autoscroll, &self.autoscroll_scripts)
        {
            let before = a.camera().0;
            let before_y = a.camera().1;
            a.step(scripts);
            self.scroll_step = (a.camera().0 - before, a.camera().1 - before_y);
            let speed = a.vx * player::UNIT;
            if self.landing.is_some() {
                // 0x08092214: until the first landing the player moves with the scroll.
                self.player.x += (a.camera().0 - before) * player::SUB;
            } else {
                self.push_along(speed);
            }
        }
        let camera_x = self.camera().0;
        self.spawner.spawn(camera_x, &self.player, &mut self.enemies, &mut self.goomba_pipes);
        for s in self.spawner.take_ship() {
            self.ship.add(&s);
        }
        if let Some(row) = self.spawner.take_autoscroll() {
            let camera_y = self.level.start_camera_y.unwrap_or(self.camera().1);
            if row >> 4 == CRUSHER_KIND {
                match &mut self.crusher {
                    Some(c) => c.restart(),
                    None => self.crusher = Some(crusher::Crusher::start(self.camera().1)),
                }
            } else {
                self.autoscroll = autoscroll::Autoscroll::start(row, camera_y);
            }
        }
        for s in self.spawner.take_fortress() {
            self.spawn_fortress(&s);
        }
        self.lifts.extend(self.spawner.take_lifts().iter().map(lifts::Lift::spawn));
        let (camera, level_h) = (self.camera(), self.level.pixel_height());
        let player = &mut self.player;
        player.riding = None;
        self.lifts.retain_mut(|l| {
            let (alive, contact) = l.step(player, camera, level_h);
            // 0x030038C5: the object the player stands on, for the crouch (0x0809F0F4).
            if contact.standing {
                player.riding = Some(l.speed());
            }
            alive
        });
        if let Some(q) = self.spawner.take_quota() {
            self.coin_quota = advance_coin::quota(&q).0;
        }
        let coins = self.spawner.take_advance_coins();
        // 0x08076C94: none once one has been hit in this level (+0xE67).
        if self.progress != 0 && !self.advance_taken {
            self.advance_coins.extend(coins.iter().map(advance_coin::AdvanceCoin::spawn));
        }
        self.step_advance_coins();
        for (i, e) in self.enemies.iter_mut().enumerate().skip(before) {
            e.init_random(self.rng.byte(i));
        }
        // Generators run before the objects (0x08090448).
        for i in 0..self.goomba_pipes.len() {
            let room = self.enemies.iter().filter(|e| e.from_pipe).count() < enemies::PIPE_GOOMBA_SLOTS;
            if let Some(e) = self.goomba_pipes[i].step(&self.player, room) {
                self.enemies.push(e);
            }
        }
        // 0x08055B34: past World 1 (0x03002A69 != 0; `status.world` counts from 1), fire
        // plants spit twice per appearance.
        let twice = self.status.world > 1;
        let (level, player, frame) = (&self.level, &self.player, self.frame);
        let camera = self.camera();
        for (i, e) in self.enemies.iter_mut().enumerate() {
            e.rand = self.rng.byte(i);
        }
        let b_held = held.contains(Buttons::B);
        self.enemies.retain_mut(|e| {
            e.spits_twice = twice;
            let gone = if e.kind == ship::BULLET_BILL { e.leaves(camera.0, frame) } else { e.from_pipe && e.off_screen(camera) };
            e.step(level, player, frame) && !gone
        });
        self.step_carried(b_held);
        for e in &self.enemies {
            if let Some(falling) = e.splash {
                // 0x0804A224: slots 1 and 2 in turn.
                self.splash_turn = self.splash_turn.wrapping_add(1);
                let slot = (self.splash_turn & 1) as usize + 1;
                let (x, y) = e.px();
                self.splashes.retain(|s| s.slot != slot);
                self.splashes.push(water::Splash::object(x, y, slot));
                if falling {
                    water::object_dive(&mut self.bubbles, x, y);
                }
            }
        }
        // 0x08055938 takes a free slot among the first four, or does not spit; 0x0808C8FC
        // gives a Micro-Goomba any free slot.
        // Puffs left by projectiles that hit a player with the star keep their slots.
        let (puff_held, puff_fire, puff_numbered) = self.puff_slots();
        for e in &mut self.enemies {
            let used =
                self.fireballs.len() + self.micro_goombas.len() + self.boomerangs.len() + self.hammers.len() + puff_held;
            let taken: Vec<u8> = self
                .boomerangs
                .iter()
                .map(|b| b.slot)
                .chain(self.hammers.iter().map(|h| h.slot))
                .chain(puff_numbered.iter().copied())
                .collect();
            // 0x08068B04 -> 0x0808C8FC: any free slot.
            if let Some(mut b) = e.thrown.take()
                && used < enemies::PROJECTILE_SLOTS
                && let Some(slot) = enemies::free_slot(taken.iter().copied())
            {
                b.slot = slot;
                self.boomerangs.push(b);
            }
            // 0x08068A84 -> 0x0808C8FC: the same.
            let taken: Vec<u8> = self
                .boomerangs
                .iter()
                .map(|b| b.slot)
                .chain(self.hammers.iter().map(|h| h.slot))
                .chain(puff_numbered.iter().copied())
                .collect();
            if let Some(mut h) = e.hammer.take()
                && used < enemies::PROJECTILE_SLOTS
                && let Some(slot) = enemies::free_slot(taken.iter().copied())
            {
                h.slot = slot;
                self.hammers.push(h);
            }
            let used =
                self.fireballs.len() + self.micro_goombas.len() + self.boomerangs.len() + self.hammers.len() + puff_held;
            if let Some(f) = e.spit.take()
                && self.fireballs.len() + puff_fire < enemies::FIREBALL_SLOTS
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
        let (enemies, player) = (&self.enemies, &self.player);
        self.boomerangs.retain_mut(|b| {
            let owner = enemies.iter().find(|e| e.id == b.owner);
            b.step(owner, player, camera, frame)
        });
        self.hammers.retain_mut(|h| {
            let owner = enemies.iter().find(|e| e.id == h.owner);
            h.step(owner, camera)
        });
        // Fire Mario's fireballs: move and bounce, then knock out what they touch
        // (0x0808B648; a fireball bursting this frame still hits everything it touches).
        let level = &self.level;
        self.player_fireballs.retain_mut(|f| f.step(level, camera) != fire::FireStep::Gone);
        for f in &mut self.player_fireballs {
            if !f.live() {
                continue;
            }
            let mut hit = false;
            for e in &mut self.enemies {
                match e.fireball_hit(f.px(), f.vx.signum()) {
                    Some(true) => {
                        hit = true;
                        let (x, y) = e.px();
                        self.status.popup(x, y, FIRE_POPUP);
                    }
                    // Fire-proof (Buzzy Beetle): the fireball just bursts (sound 0x60).
                    Some(false) => hit = true,
                    None => {}
                }
            }
            if hit {
                f.hit();
            }
        }
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
                && let Some((bump, outcome)) = blocks::hit_block(&mut self.level, &self.player, tx, ty, HitFrom::Side)
            {
                self.block_outcome(tx, ty, outcome);
                self.bumps.extend(bump);
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
        // 0x08048E8C: B held, nothing carried (0x03007848 + 0xC91), not swimming (+0x4F).
        self.player.grab = held.contains(Buttons::B) && !self.player.carrying && !self.player.in_water;
        // 0x08048960 / 0x0808CAEC: nothing touches a player hidden behind the scenery.
        let hidden = self.player.hidden;
        for e in self.enemies.iter_mut().filter(|_| !hidden) {
            match e.contact(&self.player) {
                // Stomps (0x08048B3C) and kicks from above (0x08048E58) score the chain.
                Contact::Stomped => {
                    self.player.vy = enemies::STOMP_BOUNCE;
                    self.player.on_ground = false;
                    let (x, y) = e.px();
                    self.status.chain_popup(x, y, self.player.stomp_chain + e.stomp_bonus());
                    self.player.stomp_chain += 1;
                }
                // 0x08048B3C scores a slide's stomp too (the slide cancels the bounce).
                Contact::SlideKill => {
                    let (x, y) = e.px();
                    self.status.chain_popup(x, y, self.player.stomp_chain + e.stomp_bonus());
                    self.player.stomp_chain += 1;
                }
                Contact::KickedFromAbove => {
                    self.player.kick = player::KICK_POSE_FRAMES;
                    let (x, y) = e.px();
                    self.status.chain_popup(x, y, self.player.stomp_chain);
                    self.player.stomp_chain += 1;
                }
                Contact::HurtPlayer => hurt = true,
                // 0x080488E4: the star's own chain (+0x107), 100 points up to a 1-UP.
                Contact::StarKill => {
                    let (x, y) = e.px();
                    self.status.chain_popup(x, y, self.player.star_chain.min(STAR_CHAIN_MAX));
                    self.player.star_chain += 1;
                }
                // 0x080436A0: the kick pose (+0x41 = 0x0C).
                Contact::Kicked => self.player.kick = player::KICK_POSE_FRAMES,
                Contact::Grabbed => self.player.carrying = true,
                Contact::None => {}
            }
        }
        if !hidden && self.step_fortress() {
            hurt = true;
        }
        if self.step_larry(hidden) {
            hurt = true;
        }
        // 0x0808CC9C -> 0x0808CAEC: a fireball, boomerang or hammer hurts like an enemy
        // (0x0804A5F4) and flies on; with the star (+0x3D) it turns into a puff of smoke in
        // its own slot instead (0x0808CA84, sound 0x7F).
        if !hidden {
            if self.player.star != 0 {
                self.puff_projectiles();
            } else if self.fireballs.iter().any(|f| f.touches(&self.player))
                || self.boomerangs.iter().any(|b| b.touches(&self.player))
                || self.hammers.iter().any(|h| h.touches(&self.player))
            {
                hurt = true;
            }
        }
        if self.step_ship(hidden) {
            hurt = true;
        }
        self.step_wand();
        if hurt {
            self.hurt();
        }

        self.step_goal();
        self.status.step_popups(self.frame);
        // Clock (0x080044B8); running out of time kills the player.
        // 0x080044B8 holds the clock while the player is busy (0x03003D08), e.g. growing.
        if !self.player.transforming() && !self.clock_stopped && self.landing.is_none() && self.status.tick_clock() {
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
    use enemies::EnemyState;
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

    /// A floor with a note block (0x2E) at column 6 of the ground row.
    fn note_level() -> Level {
        let (w, h) = (24, 10);
        let mut m = vec![SKY; w * h];
        for x in 0..w {
            m[(h - 1) * w + x] = 0x53;
        }
        m[(h - 1) * w + 6] = 0x2E;
        let mut rules = BlockRules::default();
        rules.bump_behaviour[0x2E] = 0;
        rules.land_behaviour[0x2E] = 0;
        rules.result_hi[0x2E] = 0x40;
        rules.final_kind[4] = 5;
        rules.final_metatile[4] = 0x2E;
        rules.bounce = [0, -64, -64, -48, -32, -16, 0, 16, 32, 48, 64];
        Level::new(w, h, m, TEST_SOLIDITY, rules)
    }

    /// Highest point (smallest y) of the player after falling onto the note block, tapping
    /// A during its bounce or not.
    fn bounce_height(press_a: bool) -> i32 {
        let mut g = Game::new();
        g.enter_level(note_level(), (6 * TILE_PX + 2, 5 * TILE_PX), &[]);
        g.screen = Screen::Level;
        let mut top = i32::MAX;
        let mut bounced = false;
        for f in 0..120 {
            // Tap A once the block is bouncing.
            let held = if press_a && !g.bumps.is_empty() && f % 2 == 0 { Buttons::A } else { Buttons::empty() };
            g.step(held);
            bounced |= !g.bumps.is_empty();
            if bounced {
                top = top.min(g.player.px().1);
            }
        }
        assert!(bounced, "the note block bounced");
        let standing = 9 * TILE_PX - g.player.height();
        assert!(top < standing - 16, "launched: {top} vs {standing}");
        top
    }

    #[test]
    fn landing_on_a_note_block_bounces_the_player_and_a_bounces_it_higher() {
        let low = bounce_height(false);
        let high = bounce_height(true);
        assert!(high < low - 32, "A during the bounce: {high} vs {low}");
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
        map.show(world_map::Arrival::Walk);
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
        let list = [sma4_assets::EnemySpawn { kind: goal::GOAL, col: 6, row: 8, ..Default::default() }];
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

    /// mGBA, World 1: A on the Toad House at (12, 3) loads level 0x102 with chest choice 7;
    /// a chest gives a mushroom, flower or leaf, and the house is done afterwards (tile 0x40).
    #[test]
    fn a_map_toad_house_gives_an_item_and_is_done_after() {
        use toad_house::{MAP_ARRIVAL, MAP_HOUSE, TOAD};
        let mut g = Game::new();
        let mut map = world_map::tests::world1();
        map.show(world_map::Arrival::Walk);
        (map.y, map.x) = (0x40, 0xC0);
        // World 1's houses: tileset 7, enemy word 0x8700 (0x08133AD0).
        for n in map.nodes.iter_mut().filter(|n| n.level == MAP_HOUSE) {
            (n.tileset, n.enemies) = (toad_house::TILESET, 0x8700);
        }
        g.world_map = Some(map);
        g.screen = Screen::Map;
        g.step(Buttons::empty());
        g.step(Buttons::A);
        for _ in 0..80 {
            g.step(Buttons::empty());
        }
        assert_eq!((g.pending_house.take(), g.house_choice), (Some(MAP_HOUSE), Some(7)));
        assert_eq!(g.pending_level, None);
        let (w, h) = (16, 27);
        let mut m = vec![SKY; w * h];
        m[24 * w..25 * w].fill(0x53);
        for y in 22..24 {
            m[y * w + 4..y * w + 11].fill(0x81);
        }
        let list = [sma4_assets::EnemySpawn { kind: TOAD, col: 0x0D, row: 0x13, ..Default::default() }];
        g.enter_map_house(Level::new(w, h, m, TEST_SOLIDITY, BlockRules::default()), &list, vec![0x10, 0xFF]);
        assert_eq!((g.screen, g.player.frame()), (Screen::Level, MAP_ARRIVAL));
        assert_eq!(g.house.as_ref().unwrap().kind, 1, "Toad's \"Pick a box\" message");
        for _ in 0..toad_house::ARRIVAL_FREEZE {
            g.step(Buttons::empty());
        }
        g.player = Player::spawn(0x40 + 2, 24 * TILE_PX - 16);
        for _ in 0..10 {
            g.step(Buttons::empty());
        }
        g.step(Buttons::B);
        // mGBA: the item in the inventory 49 frames after B, the map at 326.
        let (mut frames, mut got) = (0, None);
        while g.screen == Screen::Level {
            g.step(Buttons::empty());
            frames += 1;
            if got.is_none() && !g.inventory.is_empty() {
                got = Some(frames);
            }
            assert!(frames < 500);
        }
        assert_eq!((got, frames), (Some(51), 326));
        assert!(matches!(g.inventory.as_slice(), [1..=3]), "{:?}", g.inventory);
        assert_eq!(g.world_map.as_ref().unwrap().tile(12, 3), 0x40, "the house is done");
    }

    /// mGBA, World 1's spade: A at frame 0, the iris on Toad's room starts growing at frame
    /// 75 (sub-state 0x23) and the room's own handler (sub-state 0x24) runs from frame 107.
    #[test]
    fn the_spade_room_opens_75_frames_after_a() {
        let mut g = Game::new();
        let mut map = world_map::tests::world1();
        map.show(world_map::Arrival::Walk);
        map.nodes.push(sma4_assets::MapNode { y: 0x60, x: 0x80, tileset: spade::TILESET, level: 0x8000, enemies: 0x8001 });
        (map.y, map.x) = (0x60, 0x80);
        g.world_map = Some(map);
        g.spade_data = Some(sma4_assets::SpadeAsset::default());
        g.screen = Screen::Map;
        g.step(Buttons::empty());
        g.step(Buttons::A);
        let (mut f, mut iris_from, mut room_from) = (0, None, None);
        while room_from.is_none() {
            g.step(Buttons::empty());
            f += 1;
            let s = g.spade.as_ref();
            if iris_from.is_none() && matches!(s.map(|s| s.phase), Some(spade::Phase::Opening(k)) if k >= spade::OPEN_BLACK) {
                iris_from = Some(f);
            }
            if s.is_some_and(|s| s.phase == spade::Phase::Room) {
                room_from = Some(f);
            }
            assert!(f < 300);
        }
        assert_eq!((iris_from, room_from), (Some(75), Some(107)));
    }

    /// L opens the item panel; a mushroom makes Mario big; the Warp Whistle's whirlwind
    /// takes him to World 9 (`warp`), where he lands on its first spot.
    #[test]
    fn the_item_panel_uses_power_ups_and_the_warp_whistle() {
        let mut g = Game::new();
        let mut map = world_map::tests::world1();
        map.show(world_map::Arrival::Walk);
        g.world_map = Some(map);
        g.screen = Screen::Map;
        g.inventory = vec![inventory::MUSHROOM, inventory::WARP_WHISTLE];
        let press = |g: &mut Game, b: Buttons, wait: u32| {
            g.step(Buttons::empty());
            g.step(b);
            for _ in 0..wait {
                g.step(Buttons::empty());
            }
        };
        press(&mut g, Buttons::L, 30);
        assert_eq!(g.item_panel.as_ref().map(|p| p.state), Some(inventory::State::Menu));
        press(&mut g, Buttons::A, 30);
        assert!(g.item_panel.is_none());
        assert_eq!((g.player.form, g.inventory.as_slice()), (Form::Big, &[inventory::WARP_WHISTLE][..]));
        press(&mut g, Buttons::L, 30);
        press(&mut g, Buttons::A, 30);
        assert!(g.warp.is_some() && g.inventory.is_empty());
        let mut frames = 0;
        while g.pending_world.is_none() {
            g.step(Buttons::empty());
            frames += 1;
            assert!(frames < 600);
        }
        assert_eq!((g.pending_world.take(), g.status.world), (Some(warp::WARP_ZONE), 9));
        g.enter_world(world_map::tests::world1());
        while g.warp.is_some() {
            g.step(Buttons::empty());
            frames += 1;
            assert!(frames < 2000);
        }
        let map = g.world_map.as_ref().unwrap();
        assert_eq!((map.y, map.x), warp::LANDING);
    }

    #[test]
    fn behind_the_scenery_the_warp_trigger_leads_to_the_toad_house_and_its_chest() {
        use toad_house::{ARRIVAL, HIDDEN_HOUSE, TOAD, WARP_TRIGGER, WARP_WHISTLE};
        let mut g = Game::new();
        // 1-3's end: dark scenery (metatile 0, not sky) over the ground, the trigger ahead.
        let (w, h) = (20, 10);
        let mut m = vec![0u16; w * h];
        m[(h - 1) * w..].fill(0x53);
        let list = [sma4_assets::EnemySpawn { kind: WARP_TRIGGER, col: 10, row: 7, ..Default::default() }];
        let start = (2 * TILE_PX, 9 * TILE_PX - 16);
        g.play(Level::new(w, h, m, TEST_SOLIDITY, BlockRules::default()), start, &list, 0);
        for _ in 0..200 {
            g.step(Buttons::RIGHT);
        }
        assert_eq!(g.pending_house, None, "in front of the scenery nothing happens");
        g.play(g.level.clone(), start, &list, 0);
        g.player.behind = player::WHITE_BLOCK_TIME;
        for _ in 0..200 {
            g.step(Buttons::RIGHT);
            if g.pending_house.is_some() {
                break;
            }
        }
        assert_eq!(g.pending_house.take(), Some(HIDDEN_HOUSE));
        // The house: floor on row 24, a chest (collision ids 0x81-0x84) on rows 22-23.
        let (w, h) = (16, 27);
        let mut m = vec![SKY; w * h];
        m[24 * w..25 * w].fill(0x53);
        for y in 22..24 {
            m[y * w + 6..y * w + 9].fill(0x81);
        }
        let list = [sma4_assets::EnemySpawn { kind: TOAD, col: 0x0C, row: 0x03, ..Default::default() }];
        g.enter_house(Level::new(w, h, m, TEST_SOLIDITY, BlockRules::default()), &list, vec![0x10, 0xFF]);
        assert_eq!(g.player.frame(), ARRIVAL);
        for _ in 0..toad_house::ARRIVAL_FREEZE {
            g.step(Buttons::RIGHT);
        }
        assert_eq!(g.player.frame(), ARRIVAL, "frozen while Toad talks");
        g.player = Player::spawn(0x68 + 2, 24 * TILE_PX - 16);
        for _ in 0..10 {
            g.step(Buttons::empty());
        }
        g.step(Buttons::B);
        assert!(g.house.as_ref().unwrap().item.is_some(), "the chest opens");
        let mut frames = 0;
        let mut lid = None;
        while g.screen == Screen::Level {
            g.step(Buttons::RIGHT);
            lid = lid.or(g.house.as_ref().and_then(|h| h.chest_lid));
            frames += 1;
            assert!(frames < 400);
        }
        assert_eq!(g.inventory, [WARP_WHISTLE]);
        assert_eq!(lid, Some((0x68, 0x160)), "the chest opens");
        assert_eq!(g.player.frame().0, 0x68, "held while the item is out");
    }

    #[test]
    fn venus_fire_trap_spits_once_per_appearance_in_world_1_and_twice_later() {
        let shots = |world: u8| {
            let (w, h) = (40, 10);
            let mut kinds = vec![TileKind::Empty; w * h];
            kinds[(h - 1) * w..].fill(TileKind::Solid);
            let list = [sma4_assets::EnemySpawn { kind: enemies::FIRE_PIRANHA, col: 6, row: 6, ..Default::default() }];
            let mut g = Game::new();
            g.status.world = world;
            g.enter_level(Level::from_kinds(w, h, &kinds), (12 * TILE_PX, 9 * TILE_PX - 16), &list);
            let (mut n, mut last) = (0, 0);
            // One appearance: hidden 0x60, up, out 0x60, down.
            for _ in 0..0xF0 {
                g.invulnerable = 2;
                g.step(Buttons::empty());
                n += g.fireballs.len().saturating_sub(last);
                last = g.fireballs.len();
            }
            n
        };
        assert_eq!(shots(1), 1);
        assert_eq!(shots(2), 2);
    }

    /// A Hammer Bro battle like World 1's arena 0x4E: one screen, ground on row 26, bricks
    /// on row 22 (columns 7-13), the Bro at (12, 24) and level effect 7 (list type 0x166).
    fn battle() -> Game {
        let (w, h) = (16, 27);
        let mut kinds = vec![TileKind::Empty; w * h];
        kinds[26 * w..].fill(TileKind::Solid);
        kinds[22 * w + 7..22 * w + 14].fill(TileKind::Solid);
        let list = [
            sma4_assets::EnemySpawn { kind: enemies::HAMMER_BRO, col: 12, row: 24, ..Default::default() },
            sma4_assets::EnemySpawn { kind: fortress::CHEST_EXIT, col: 13, row: 20, ..Default::default() },
        ];
        let mut g = Game::new();
        g.enter_level(Level::from_kinds(w, h, &kinds), (2 * TILE_PX, 26 * TILE_PX - 16), &list);
        g.set_time_limit(200);
        g.battle = Some(9);
        g
    }

    #[test]
    fn a_projectile_hitting_the_star_turns_into_a_puff_that_holds_its_slot() {
        let mut g = battle();
        g.enemies.clear();
        g.player.star = 0xFF;
        g.camera_zones.settle(&g.player);
        let form = g.player.form;
        let (fx, fy) = g.player.frame();
        // A fireball on the player's frame (0x0808CC9C: x - frame x + 6 < 0x10, y 0x12 down).
        g.fireballs.push(enemies::Fireball { x: fx * player::SUB, y: (fy + 0x12) * player::SUB, vx: 0, vy: 0, age: 0 });
        g.step(Buttons::empty());
        assert!(g.fireballs.is_empty(), "the slot became the puff");
        assert_eq!(g.puffs.len(), 1);
        let p = g.puffs[0];
        assert_eq!((p.x, p.y, p.life, p.slot), (fx, fy + 0x12, fortress::PUFF_LIFE, fortress::PuffSlot::Fireball));
        assert_eq!(g.player.form, form, "no hit");
        // 0x0808F568: drawn with +0x14 = 0x16 down to 0, then the slot is free.
        let mut shown = 0;
        while !g.puffs.is_empty() {
            g.step(Buttons::empty());
            shown += g.puffs.len();
        }
        assert_eq!(shown, 0x17);
        // Without the star it hurts and flies on.
        g.player.star = 0;
        g.fireballs.push(enemies::Fireball { x: fx * player::SUB, y: (fy + 0x12) * player::SUB, vx: 0, vy: 0, age: 0 });
        g.step(Buttons::empty());
        assert!(g.puffs.is_empty());
        assert_eq!(g.fireballs.len(), 1);
        assert!(g.invulnerable > 0 || g.dying.is_some(), "hurt");
    }

    #[test]
    fn the_hammer_bro_throws_hammers_and_its_chest_comes_once_it_is_gone() {
        let mut g = battle();
        let mut thrown = 0;
        for _ in 0..300 {
            g.invulnerable = 100;
            let before = g.hammers.len();
            g.step(Buttons::empty());
            thrown += g.hammers.len().saturating_sub(before);
            assert!(g.chest.is_none(), "not while the Bro is there");
        }
        assert!(g.enemies.iter().any(|e| e.kind == enemies::HAMMER_BRO));
        assert!(thrown >= 3, "it throws hammers ({thrown})");
        // Knocked out: once its hammers have gone too, the chest drops on the far side
        // (the player is left of x 0x80: camera + 0xC0, y 0x170, 0x0804D12C).
        g.enemies.clear();
        for _ in 0..200 {
            g.invulnerable = 100;
            g.step(Buttons::empty());
            if g.chest.is_some() {
                break;
            }
        }
        assert!(g.hammers.is_empty());
        let c = g.chest.expect("a chest");
        assert_eq!(c.px(), (0xC0, 0x170));
        // Opened: its item (the map object's, a Starman) goes into the inventory and the
        // level ends 0x60 frames later.
        let mut got = false;
        for _ in 0..400 {
            g.invulnerable = 100;
            g.step(if g.player.px().0 < 0xB8 { Buttons::RIGHT } else { Buttons::empty() });
            got |= g.inventory.contains(&9);
            if g.screen == Screen::Map {
                break;
            }
        }
        assert!(got, "the Starman");
        assert_eq!(g.screen, Screen::Map);
        assert_eq!(g.battle, None);
    }

    /// A flat arena 0x100 px into a 0xA00 px level, Boom Boom in it.
    fn arena() -> Game {
        let (w, h) = (160, 27);
        let mut kinds = vec![TileKind::Empty; w * h];
        kinds[25 * w..].fill(TileKind::Solid);
        let list = [sma4_assets::EnemySpawn { kind: boom_boom::BOOM_BOOM, col: 156, row: 23, ..Default::default() }];
        let mut g = Game::new();
        g.enter_level(Level::from_kinds(w, h, &kinds), (130 * TILE_PX, 25 * TILE_PX - 16), &list);
        g.set_time_limit(300);
        g
    }

    #[test]
    fn the_star_bursts_boom_boom_at_once_and_the_clock_stops() {
        let mut g = arena();
        for _ in 0..600 {
            g.invulnerable = 100;
            g.step(if g.camera().0 < 0x8F0 { Buttons::RIGHT } else { Buttons::empty() });
        }
        assert!(g.boom_boom.is_some_and(|b| b.phase >= 2));
        let b = g.boom_boom.unwrap();
        let (bx, by) = b.px();
        g.player = Player::spawn(bx + 10, by + 12);
        g.player.star = 100;
        g.step(Buttons::empty());
        assert!(g.boom_boom.is_none() && g.ball.is_some(), "burst at once");
        let clock = g.status.clock;
        for _ in 0..120 {
            g.step(Buttons::empty());
        }
        assert_eq!(g.status.clock, clock, "the clock holds after the burst");
    }

    #[test]
    fn boom_boom_locks_the_arena_takes_three_stomps_and_leaves_the_ball() {
        let mut g = arena();
        // Walk in until the camera locks and the intro runs.
        for _ in 0..600 {
            g.invulnerable = 100;
            g.step(if g.camera().0 < 0x8F0 { Buttons::RIGHT } else { Buttons::empty() });
        }
        let b = g.boom_boom.expect("Boom Boom spawned");
        assert!(b.phase >= 2, "the fight started (phase {})", b.phase);
        assert_eq!(g.camera().0, 0x908, "camera locked after the slide");
        assert_eq!(g.level.metatile(144, 21), Some(boom_boom::INTRO_BLOCK), "the arena is closed behind the player");
        let mut stomps = 0;
        for _ in 0..4000 {
            let Some(b) = g.boom_boom else { break };
            // Drop onto him whenever he can be stomped.
            if b.dazed == 0 && b.phase != 5 && g.player.vy >= 0 {
                let (bx, by) = b.px();
                g.player = Player::spawn(bx + 10, by - 4);
                g.player.vy = 0x20;
            }
            g.invulnerable = 100;
            let before = g.boom_boom.map(|b| b.hp);
            g.step(Buttons::empty());
            if g.boom_boom.is_some() && g.boom_boom.map(|b| b.hp) != before {
                stomps += 1;
            }
        }
        assert_eq!(stomps, 3);
        assert!(g.boom_boom.is_none() && g.ball.is_some(), "he burst into the ball");
        // Touch the ball once it falls.
        for _ in 0..200 {
            if let Some(ball) = g.ball {
                let (x, y) = ball.px();
                g.player = Player::spawn(x + 2, y);
            }
            g.step(Buttons::empty());
        }
        assert!(g.fortress_clear.is_some(), "touching the ball clears the fortress");
        let score = g.status.score;
        for _ in 0..2000 {
            g.step(Buttons::empty());
        }
        assert!(g.status.score > score, "the clock is counted into the score");
        assert_eq!(g.screen, Screen::Map);
    }

    #[test]
    fn advance_coin_needs_progress_and_a_head_hit_then_counts_the_coins_to_go() {
        let (w, h) = (30, 12);
        let mut kinds = vec![TileKind::Empty; w * h];
        kinds[(h - 1) * w..].fill(TileKind::Solid);
        let list = [
            sma4_assets::EnemySpawn { kind: advance_coin::QUOTA, col: 1, row: 44, ..Default::default() },
            sma4_assets::EnemySpawn { kind: advance_coin::COIN, col: 7, row: 6, ..Default::default() },
        ];
        let start = (7 * TILE_PX + 2, 11 * TILE_PX - 32);
        // A new game (progress 0) never shows it.
        let mut g = Game::new();
        g.enter_level(Level::from_kinds(w, h, &kinds), start, &list);
        for _ in 0..10 {
            g.step(Buttons::empty());
        }
        assert!(g.advance_coins.is_empty());
        assert_eq!(g.coin_quota, 44, "the quota entry is read either way");
        // Beaten: the coin is there, its cell solid; a jump into it sets it off.
        let mut g = Game::new();
        g.progress = 1;
        g.enter_level(Level::from_kinds(w, h, &kinds), start, &list);
        g.player.set_form(Form::Big);
        g.step(Buttons::empty());
        g.step(Buttons::empty());
        assert_eq!(g.advance_coins.len(), 1);
        assert_eq!(g.level.at(7, 6), TileKind::Solid);
        let mut jumped = false;
        for f in 0..400 {
            let held = if (15..35).contains(&f) { Buttons::A } else { Buttons::empty() };
            g.step(held);
            jumped |= g.advance_coins[0].freezes();
        }
        assert!(jumped, "the head hit freezes the game while the coin flies");
        assert_eq!(g.advance_coins[0].phase(), advance_coin::Phase::Counting);
        assert_eq!(g.advance_coins[0].remaining(g.level_coins, g.coin_quota), [0, 4, 4]);
    }

    /// World 1-4 in mGBA: the coin counter, the quota and the hit flag (+0xE67) survive an
    /// area switch; a hit coin does not come back in the same level.
    #[test]
    fn advance_coin_state_belongs_to_the_level_not_the_area() {
        let (w, h) = (30, 12);
        let mut kinds = vec![TileKind::Empty; w * h];
        kinds[(h - 1) * w..].fill(TileKind::Solid);
        let level = || Level::from_kinds(w, h, &kinds);
        let list = [
            sma4_assets::EnemySpawn { kind: advance_coin::QUOTA, col: 1, row: 44, ..Default::default() },
            sma4_assets::EnemySpawn { kind: advance_coin::COIN, col: 7, row: 6, ..Default::default() },
        ];
        let mut g = Game::new();
        g.progress = 1;
        g.enter_level(level(), (7 * TILE_PX + 2, 11 * TILE_PX - 32), &list);
        g.player.set_form(Form::Big);
        for f in 0..300 {
            g.step(if (15..35).contains(&f) { Buttons::A } else { Buttons::empty() });
        }
        assert!(g.advance_taken);
        g.level_coins = 7;
        let to = pipes::AreaRequest {
            level: 0,
            enemies: 0,
            tileset: 0,
            exit: sma4_assets::PipeExit { page: 0, x: 0x40, y: 0x40, dir: pipes::SKY_EXIT, camera_y: 0 },
        };
        g.enter_area(level(), &list, &to);
        for _ in 0..5 {
            g.step(Buttons::empty());
        }
        assert_eq!((g.level_coins, g.coin_quota, g.advance_taken), (7, 44, true));
        assert!(g.advance_coins.is_empty(), "0x08076C94 removes a coin once one was hit");
        // Starting the level again (from the map, or after a lost life) clears it all.
        g.enter_level(level(), (7 * TILE_PX + 2, 11 * TILE_PX - 32), &list);
        g.step(Buttons::empty());
        assert_eq!((g.level_coins, g.advance_taken, g.advance_coins.len()), (0, false, 1));
    }

    /// The head hit counts anywhere the ROM's touch boxes overlap (mGBA: frame x 100-124
    /// for big Mario under World 1-4's coin at x 112), not only under the head's centre.
    #[test]
    fn advance_coin_is_hit_off_centre() {
        let (w, h) = (30, 12);
        let mut kinds = vec![TileKind::Empty; w * h];
        kinds[(h - 1) * w..].fill(TileKind::Solid);
        let list = [sma4_assets::EnemySpawn { kind: advance_coin::COIN, col: 7, row: 6, ..Default::default() }];
        for fx in [100, 104, 120, 124] {
            let mut g = Game::new();
            g.progress = 1;
            g.enter_level(Level::from_kinds(w, h, &kinds), (fx + 2, 11 * TILE_PX - 32), &list);
            g.player.set_form(Form::Big);
            let mut hit = false;
            for f in 0..60 {
                g.step(if (15..35).contains(&f) { Buttons::A } else { Buttons::empty() });
                hit |= g.advance_taken;
            }
            assert!(hit, "frame x {fx}");
        }
    }

    #[test]
    fn world_1_4_scrolls_by_itself_pushes_the_player_and_its_lifts_fall_when_stood_on() {
        // Ground on the last row; the autoscroll entry at column 0 and a lift over a gap.
        let (w, h) = (60, 12);
        let mut kinds = vec![TileKind::Empty; w * h];
        kinds[(h - 1) * w..(h - 1) * w + 12].fill(TileKind::Solid);
        let list = [
            sma4_assets::EnemySpawn { kind: autoscroll::AUTOSCROLL, col: 0, row: 0, ..Default::default() },
            sma4_assets::EnemySpawn { kind: lifts::LIFT, col: 20, row: 8, ..Default::default() },
        ];
        let mut g = Game::new();
        g.autoscroll_scripts = Some(autoscroll::tests::scripts());
        g.enter_level(Level::from_kinds(w, h, &kinds), (2 * TILE_PX, 11 * TILE_PX - 16), &list);
        for _ in 0..300 {
            g.step(Buttons::empty());
        }
        let (cx, _) = g.camera();
        assert!(cx > 100, "the camera moved on its own: {cx}");
        assert!(g.player.frame().0 >= cx, "the player is pushed along, not left behind");
        assert!(g.lifts[0].px().0 < 20 * TILE_PX - 50, "the lift drifts left");
        assert!(!g.lifts[0].falling());
        // Stand on it: it falls, carrying the player.
        let (lx, ly) = g.lifts[0].px();
        g.player = Player::spawn(lx + 16, ly - 16 - 4);
        let mut rode = 0;
        for _ in 0..20 {
            g.step(Buttons::empty());
            if g.player.on_ground && g.player.frame().1 == g.lifts[0].px().1 - 0x1F {
                rode += 1;
            }
        }
        assert!(g.lifts[0].falling());
        assert!(g.lifts[0].px().1 > ly + 5);
        assert!(rode > 10, "the player rides it down ({rode} frames)");
    }

    #[test]
    fn the_course_clear_walk_carries_on_off_the_end_of_the_ground() {
        // World 1-2: slope collision, and the ground ends before the player is off the
        // screen, so he walks off it and must keep going through the air (+0x43).
        let mut level = crate::level::test_hill();
        for x in 17..24 {
            level.set(x, 7, 0x80);
        }
        let mut g = Game::new();
        let list = [sma4_assets::EnemySpawn { kind: goal::GOAL, col: 1, row: 6, ..Default::default() }];
        g.enter_level(level, (TILE_PX, 7 * TILE_PX - 16), &list);
        g.set_time_limit(100);
        g.screen = Screen::Level;
        g.step(Buttons::empty());
        assert!(g.goal_camera.is_some(), "touched");
        let mut frames = 0;
        while g.screen == Screen::Level {
            g.step(Buttons::empty());
            frames += 1;
            assert!(frames < 3000, "stuck at {:?}", g.player.px());
        }
        assert_eq!(g.screen, Screen::Map);
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
        let list = [sma4_assets::EnemySpawn { kind: goal::GOAL, col: 6, row: 8, ..Default::default() }];
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

    /// A level with a floor and one Goomba to the right of the player.
    fn goomba_ahead() -> Game {
        let mut g = Game::new();
        let list = [sma4_assets::EnemySpawn { kind: enemies::WALKER, col: 10, row: 8, ..Default::default() }];
        g.enter_level(block_level((20, 2)), (2 * TILE_PX, 9 * TILE_PX - 16), &list);
        g.screen = Screen::Level;
        g
    }

    #[test]
    fn fire_mario_throws_two_fireballs_at_most_and_they_knock_out_a_goomba() {
        let mut g = goomba_ahead();
        g.player.set_form(Form::Fire);
        g.step(Buttons::empty());
        assert_eq!(g.enemies.len(), 1);
        for _ in 0..3 {
            g.step(Buttons::B);
            g.step(Buttons::empty());
        }
        assert_eq!(g.player_fireballs.len(), 2, "two slots");
        let score = g.status.score;
        let mut killed = false;
        for _ in 0..150 {
            g.step(Buttons::empty());
            killed |= g.enemies.first().is_some_and(|e| e.state == EnemyState::Dead);
        }
        assert!(killed, "knocked out");
        assert_eq!(g.status.score, score + 10, "100 points");
    }

    #[test]
    fn with_the_star_touching_an_enemy_knocks_it_out_and_hits_do_not_count() {
        let mut g = goomba_ahead();
        g.player.starman();
        let mut killed = false;
        for _ in 0..200 {
            g.step(Buttons::RIGHT);
            killed |= g.enemies.iter().any(|e| e.state == EnemyState::Dead);
        }
        assert!(killed, "the Goomba was knocked out");
        assert_eq!(g.player.star_chain, 1);
        assert_eq!(g.player.form, Form::Small, "no hit taken");
        assert!(g.dying.is_none());
    }

    #[test]
    fn sliding_into_an_enemy_knocks_it_out_without_a_bounce() {
        // Checked in mGBA (World 1-1's first Goomba, slide byte poked to 0x30): it flies off
        // for 100 points and Mario slides on.
        let mut g = goomba_ahead();
        for _ in 0..3 {
            g.step(Buttons::empty());
        }
        g.player.slide = 0x40;
        let score = g.status.score;
        let mut killed = false;
        for _ in 0..80 {
            g.step(Buttons::empty());
            assert!(g.player.on_ground && g.player.vy == 0, "no bounce");
            killed |= g.enemies.iter().any(|e| e.state == EnemyState::Dead && e.vy < 0);
        }
        assert!(killed, "the Goomba was knocked out");
        assert_eq!(g.player.form, Form::Small, "no hit taken");
        assert!(g.dying.is_none());
        assert_eq!(g.status.score, score + 10, "100 points");
    }

    #[test]
    fn a_fire_flower_from_a_block_makes_a_big_player_fire_mario() {
        let block = (4, 5);
        let mut g = Game::new();
        let mut level = block_level(block);
        level.rules.item_types[2] = blocks::FIRE_FLOWER;
        level.rules.bump_behaviour[0x61] = 1;
        g.enter_level(level, (block.0 as i32 * TILE_PX + 2, 9 * TILE_PX - 16), &[]);
        g.screen = Screen::Level;
        g.player.set_form(Form::Big);
        for _ in 0..60 {
            g.step(Buttons::empty());
        }
        for f in 0..120 {
            g.step(if f < 12 { Buttons::A } else { Buttons::empty() });
        }
        // It rose out of the block and stays on top of it.
        let flower = g.items.first().copied().expect("a flower");
        assert_eq!(flower.kind, blocks::FIRE_FLOWER);
        assert_eq!(flower.px(), (block.0 as i32 * TILE_PX, (block.1 as i32 - 1) * TILE_PX));
        // Stand on the block next to it.
        g.player = Player::spawn(flower.px().0 + 4, flower.px().1 + 16 - Form::Big.height());
        g.player.set_form(Form::Big);
        g.step(Buttons::empty());
        assert_eq!((g.player.form, g.player.flash), (Form::Fire, player::FIRE_FLASH));
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

    /// A game with a Koopa shell resting on the ground 2 tiles right of the player.
    fn shell_game() -> Game {
        let mut g = Game::new();
        g.enter_level(block_level((20, 2)), (2 * TILE_PX, 9 * TILE_PX - 16), &[]);
        for _ in 0..30 {
            g.step(Buttons::empty());
        }
        let mut shell = enemies::Enemy::spawn_at(enemies::KOOPA, 4 * TILE_PX, 8 * TILE_PX, &g.player);
        shell.state = EnemyState::Shell(0xFF);
        shell.vx = 0;
        g.enemies = vec![shell];
        g
    }

    fn walk_into_shell(g: &mut Game, held: Buttons) {
        for _ in 0..90 {
            g.step(held);
            if g.enemies[0].state != EnemyState::Shell(0xFF) && !matches!(g.enemies[0].state, EnemyState::Shell(_)) {
                break;
            }
        }
    }

    /// mGBA (1-1, a Koopa poked into a shell): walking into it with B held picks it up; it
    /// sits 0xB px ahead of the frame and 0xE below its top, Mario shows the carrying
    /// poses; letting go of B kicks it away with the kick pose for 12 frames.
    #[test]
    fn b_held_picks_up_a_shell_and_letting_go_kicks_it() {
        let mut g = shell_game();
        walk_into_shell(&mut g, Buttons::B | Buttons::RIGHT);
        assert!(matches!(g.enemies[0].state, EnemyState::Carried(_)));
        assert!(g.player.carrying);
        for _ in 0..10 {
            g.step(Buttons::B | Buttons::RIGHT);
            let (fx, fy) = g.player.frame();
            assert_eq!(g.enemies[0].px(), (fx + 0x0B, fy + 0x0E));
            assert!(matches!(g.player.pose(), player::Pose::Hold(_)), "{:?}", g.player.pose());
        }
        g.step(Buttons::empty());
        assert_eq!(g.enemies[0].state, EnemyState::Kicked);
        assert!(g.enemies[0].vx > 0, "away from the player");
        assert!(!g.player.carrying);
        let mut kick = 0;
        for _ in 0..20 {
            g.step(Buttons::empty());
            kick += (g.player.pose() == player::Pose::Kick) as u32;
        }
        assert_eq!(kick, 12);
    }

    /// Without B the shell is kicked as before; turning round with it shows the turn pose
    /// for 7 frames and swings it round (0x0816943E: 4 px, then -4).
    #[test]
    fn turning_round_with_a_shell() {
        let mut g = shell_game();
        walk_into_shell(&mut g, Buttons::RIGHT);
        assert_eq!(g.enemies[0].state, EnemyState::Kicked);
        let mut g = shell_game();
        walk_into_shell(&mut g, Buttons::B | Buttons::RIGHT);
        for _ in 0..30 {
            g.step(Buttons::B);
        }
        let mut turn = Vec::new();
        for _ in 0..40 {
            g.step(Buttons::B | Buttons::LEFT);
            let fx = g.player.frame().0;
            turn.push((g.player.pose() == player::Pose::HoldTurn, g.enemies[0].px().0 - fx));
            if !g.player.facing_left {
                turn.clear();
            }
        }
        let frames: Vec<i32> = turn.iter().filter(|t| t.0).map(|t| t.1).collect();
        assert_eq!(frames, [4, 4, -4, -4, -4, -4, -0x0B]);
        assert_eq!(turn.last().unwrap().1, -0x0B);
    }

    /// The carried shell knocks out what it touches, and goes too (0x08048868).
    #[test]
    fn a_carried_shell_knocks_out_an_enemy_and_itself() {
        let mut g = shell_game();
        walk_into_shell(&mut g, Buttons::B | Buttons::RIGHT);
        let goomba = enemies::Enemy::spawn_at(enemies::WALKER, g.player.px().0 + 40, 8 * TILE_PX, &g.player);
        g.enemies.push(goomba);
        for _ in 0..30 {
            g.step(Buttons::B | Buttons::RIGHT);
        }
        assert!(g.enemies.iter().all(|e| e.state == EnemyState::Dead), "{:?}", g.enemies.iter().map(|e| e.state).collect::<Vec<_>>());
        assert!(!g.player.carrying);
        assert_eq!(g.status.popups.len(), 1);
    }

    /// Held too long, the Koopa comes out (0x08044720) and walks toward the player.
    #[test]
    fn a_shell_held_too_long_wakes_up() {
        let mut g = shell_game();
        walk_into_shell(&mut g, Buttons::B | Buttons::RIGHT);
        g.enemies[0].state = EnemyState::Carried(2);
        g.step(Buttons::B);
        g.step(Buttons::B);
        assert_eq!(g.enemies[0].state, EnemyState::Walking);
        assert!(!g.player.carrying);
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
    fn quit_fades_out_to_the_title_which_starts_a_new_game() {
        let mut g = Game::new();
        g.title_data = Some(std::sync::Arc::new(sma4_assets::TitleAsset::default()));
        g.enter_level(block_level((20, 2)), (8 * TILE_PX, 9 * TILE_PX - 16), &[]);
        g.status.lives = 0;
        g.status.score = 500;
        g.die();
        while !matches!(g.screen, Screen::GameOverMenu(_)) {
            g.step(Buttons::empty());
        }
        for _ in 0..0x10 {
            g.step(Buttons::empty());
        }
        g.step(Buttons::DOWN);
        g.step(Buttons::empty());
        g.step(Buttons::A);
        // Sub-state 8: 8 palette steps every other frame, the 8th on the 15th frame.
        let mut fades = Vec::new();
        while let Screen::GameOverMenu(m) = g.screen {
            fades.push(m.fade().expect("fading"));
            g.step(Buttons::empty());
        }
        assert_eq!(fades, [0, 4, 4, 8, 8, 12, 12, 16, 16, 20, 20, 24, 24, 28, 28]);
        assert_eq!(g.screen, Screen::Title);
        // The title: Start skips the scene once it is up, A opens the file select, A on File
        // 1 the mode select, A starts.
        let mut started = None;
        for f in 0..400 {
            let t = g.title.as_ref().map(|t| t.phase);
            let keys = if f % 20 == 19 { if t == Some(0) || t.is_none() { Buttons::START } else { Buttons::A } } else { Buttons::empty() };
            g.step(keys);
            if g.screen == Screen::Map {
                started = Some(f);
                break;
            }
        }
        assert!(started.is_some(), "a new game on the map");
        assert_eq!((g.status.lives, g.status.score), (status::START_LIVES, 0));
        assert!(g.title.is_none());
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

    #[test]
    fn the_spade_spot_plays_the_bonus_game_then_is_cleared() {
        let mut g = Game::new();
        let mut map = world_map::tests::world1();
        map.nodes.push(sma4_assets::MapNode { y: 0x60, x: 0x80, tileset: 15, level: 0x8000, enemies: 0x8001 });
        (map.y, map.x) = (0x60, 0x80);
        g.world_map = Some(map);
        g.screen = Screen::Map;
        g.spade_data = Some(spade::tests::data());
        g.status.lives = 4;
        g.step(Buttons::A);
        let mut frames = 0;
        while g.screen == Screen::Map {
            g.step(Buttons::empty());
            frames += 1;
            assert!(frames < 100, "the iris closes, then the game");
        }
        assert_eq!(g.screen, Screen::Spade);
        assert!(g.pending_level.is_none(), "no level to load");
        // Tap A until the rows have stopped and the game is over.
        let mut frames = 0;
        while g.screen == Screen::Spade {
            g.step(if frames % 30 == 0 { Buttons::A } else { Buttons::empty() });
            frames += 1;
            assert!(frames < 3000);
        }
        assert_eq!(g.screen, Screen::Map);
        let m = g.world_map.as_ref().unwrap();
        assert_eq!(m.tile(8, 5), 0xC0, "the spade's done tile");
        assert_eq!(m.phase, MapPhase::Idle);
        assert_eq!(m.checkpoint, (0x40, 0x20), "not a checkpoint (tileset 15)");
        assert!(g.status.lives == 4 || g.status.lives >= 6, "lost, or 2 lives or more won");
    }

    #[test]
    fn larry_beaten_by_three_stomps_flies_off_and_his_wand_comes_down() {
        let mut g = Game::new();
        let list = [sma4_assets::EnemySpawn { kind: larry::LARRY, col: 10, row: 7, ..Default::default() }];
        g.enter_level(block_level((20, 2)), (0x20, 9 * TILE_PX - 16), &list);
        g.set_time_limit(300);
        let (mut cooldown, mut stomps, mut drawn) = (0u32, 0, false);
        for _ in 0..3000 {
            g.invulnerable = 100;
            if let Some(l) = &g.larry {
                drawn |= l.state == 0 && g.obj_sprites.iter().any(|s| s.pal == larry::PALETTE);
                if l.state == 0 && l.shell == 0 && l.magic == 0 && cooldown == 0 {
                    // Dropped onto him from a little above.
                    let (lx, ly) = l.px();
                    g.player.set_frame_sub((lx + 4) * player::SUB, (ly - 0x18) * player::SUB);
                    g.player.vy = 0x20 * player::UNIT;
                    g.player.on_ground = false;
                    cooldown = 40;
                }
            }
            cooldown = cooldown.saturating_sub(1);
            let hits = g.larry.as_ref().map_or(0, |l| l.hits);
            g.step(Buttons::empty());
            if g.larry.as_ref().is_some_and(|l| l.hits > hits) {
                stomps += 1;
                assert!(g.player.vy < 0, "the player bounces off him");
            }
            if g.wand.is_some() {
                break;
            }
        }
        assert!(drawn, "his sprites");
        assert_eq!(stomps, 3);
        assert!(g.clock_stopped, "the third stomp stops the clock");
        assert_eq!(g.larry.as_ref().map(|l| l.state), Some(2), "flew off the top");
        assert!(g.wand.is_some(), "the wand falls");
    }

    #[test]
    fn catching_the_wand_holds_mario_and_counts_the_clock_at_200_points_a_second() {
        let mut g = Game::new();
        // Standing where the wand comes down (x 0x80).
        g.enter_level(block_level((20, 2)), (0x7C, 9 * TILE_PX - 16), &[]);
        g.set_time_limit(100);
        g.drop_wand();
        for _ in 0..200 {
            g.step(Buttons::empty());
            if g.wand_clear.is_some() {
                break;
            }
        }
        assert!(g.wand.is_none() && g.wand_clear.is_some(), "caught");
        let kept = g.status.clock;
        let (pos, score) = (g.player.px(), g.status.score);
        let mut frames = 0;
        let mut states = [0u32; 8];
        while g.screen == Screen::Level {
            let fy = g.player.frame().1;
            let was = g.wand_clear.map_or(0, |c| c.state);
            g.step(Buttons::RIGHT | Buttons::A);
            frames += 1;
            let Some(c) = g.wand_clear else { break };
            states[c.state as usize] += 1;
            assert_eq!(g.player.forced_pose, Some(player::Pose::Jump));
            assert_eq!(g.status.bar_clock, kept, "the bar keeps the stopped clock");
            if was < 7 {
                assert_eq!(g.player.px(), pos, "held");
                assert_eq!(g.obj_front, 2, "the wand over his head");
                assert!(g.obj_sprites[..2].iter().all(|s| s.tile == 0x97));
            }
            if c.state < 7 {
                assert_eq!(g.obj_front, 2);
            }
            if was == 7 {
                assert_eq!(g.player.frame().1, fy + wand::FALL_STEP, "falling");
                assert!(g.level_hidden && g.backdrop.is_some());
            }
            assert!(frames < 2000);
        }
        // States 3-4: 0x150 frames of delay, a drain step per ten seconds and per second,
        // 0x30 + 1 frames after (mGBA, 287 s: 420 frames from the catch); state 5 0xC0
        // frames, state 6 0xFF, state 7 until the frame y reaches 0x200.
        let steps = kept[0] as u32 * 10 + kept[1] as u32 + kept[2] as u32;
        assert_eq!(states[3] + states[4] + 1, 385 + steps);
        assert_eq!((states[5], states[6]), (0xC0, 0xFF));
        assert_eq!(g.player.frame().1, 0x200);
        assert!(g.wand_clear.is_none() && g.backdrop.is_none());
        let seconds = kept[0] as u32 * 100 + kept[1] as u32 * 10 + kept[2] as u32;
        assert_eq!(g.status.score - score, seconds * 20);
        assert_eq!(g.status.clock, [0, 0, 0]);
    }

    #[test]
    fn a_fireball_bumps_the_resting_wand_three_frames_after_the_throw() {
        let mut g = Game::new();
        g.enter_level(block_level((20, 2)), (0x40, 9 * TILE_PX - 16), &[]);
        g.player.form = Form::Fire;
        g.drop_wand();
        for _ in 0..250 {
            g.step(Buttons::empty());
        }
        let w = g.wand.unwrap();
        assert_eq!(w.vy, 0, "resting");
        // mGBA (cabin): Fire Mario standing at x 0x8E (the wand at 0x80) facing left throws
        // on frame 0; the wand is bumped during frame 3 (speed -32, then -31 after its
        // move), sparkles appear from frame 4; the fireball is a puff (type 4) where it was.
        let (fx, fy) = g.player.frame();
        g.player.set_frame_sub(((w.x >> 8) + 0x0E) * player::SUB, fy * player::SUB);
        g.player.facing_left = true;
        assert_eq!(g.player.frame(), ((w.x >> 8) + 0x0E, fy), "{fx}");
        let mut bumped = None;
        for f in 0..12 {
            g.step(if f < 2 { Buttons::B } else { Buttons::empty() });
            if bumped.is_none() && g.wand.unwrap().vy < 0 {
                bumped = Some(f);
                assert_eq!(g.wand.unwrap().vy, -31);
                assert!(!g.player_fireballs[0].live());
            }
        }
        assert_eq!(bumped, Some(3));
        // A sparkle on frames with (counter & 7) == 0 while it flies.
        assert!(!g.wand_sparkles.is_empty() || g.obj_sprites.iter().any(|s| (0x68..=0x6C).contains(&s.tile)));
    }


    #[test]
    fn the_tail_bumps_the_resting_wand_on_the_fourth_frame_of_the_whip() {
        let mut g = Game::new();
        g.enter_level(block_level((20, 2)), (0x40, 9 * TILE_PX - 16), &[]);
        g.player.form = Form::Raccoon;
        g.drop_wand();
        for _ in 0..250 {
            g.step(Buttons::empty());
        }
        let w = g.wand.unwrap();
        // mGBA (cabin): Raccoon Mario at x 0x73 (the wand at 0x80) facing left whips on
        // frame 0; the wand is bumped during frame 4 (+0x15 = 8).
        let (_, fy) = g.player.frame();
        g.player.set_frame_sub(((w.x >> 8) - 0x0D) * player::SUB, fy * player::SUB);
        g.player.facing_left = true;
        let mut bumped = None;
        for f in 0..20 {
            g.step(if f < 2 { Buttons::B } else { Buttons::empty() });
            if bumped.is_none() && g.wand.unwrap().vy < 0 {
                bumped = Some(f);
                assert_eq!(g.wand.unwrap().vy, -31);
            }
            // mGBA: the hit star (tile 0x8B, OAM 0 / 1) in frames 4..13.
            let star = g.obj_sprites.len() >= 2 && g.obj_sprites[..2].iter().all(|o| o.tile == 0x8B);
            assert_eq!(star, (4..14).contains(&f), "frame {f}");
        }
        assert_eq!(bumped, Some(4));
        assert!(g.wand.unwrap().vy > -31, "once: the tail waits 8 frames, then it is out of reach");
    }

}
