//! The world map: Mario walking from spot to spot and entering levels, reimplemented from
//! the map loop (mode 1 sub-state 0x11, 0x0800DE10; see docs/re-notes.md, "World map
//! walking").
//!
//! * Map state (*0x03007824, per player): +0 y, +2 screen, +4 x, +6 walk counter, +8
//!   direction (keys >> 4), +0x66 move cooldown; the last move per world is 0x03002D65.
//! * Tiles: 16 columns per screen, row = (y - 0x10) >> 4 (0x08010C8C).
//! * Idle (map sub-state 0x0E, 0x0801112C): a D-pad press walks two cells if the cell next
//!   to Mario in that direction is a path tile for it (0x08010E20 / 0x08010D18): 2 px per
//!   frame for 16 frames (walk counter 0x20, -2 per frame). Standing on a spot that can be
//!   entered, Mario may only go back the way he came. A on such a spot enters it (map
//!   sub-state 0x17); the iris closes (0x0802F44C) and the level loads.
//! * Cleared spots change tile (0x08009428): panels become 0x00, Toad houses 0x40, the
//!   spade 0xC0, the fortress 0x60 (and its lock opens).
//! * Checkpoint (0x030037B1 / B3 / B5): START when the world begins; clearing a level
//!   (0x080270B8, any tileset below 0x0F except Toad houses, 7) moves it to that level's
//!   spot. After a game over Mario flies back to it (0x080131EC) and the Toad houses and
//!   spade panels come back (0x08012458, mask 0x0813C12E).
//! * After a lost life (sub-state 2, the panel) Mario goes back to the checkpoint in 0x20
//!   frames (sub-state 3, 0x0800ECFC), then the map objects move (0x0C; `map_objects`). A
//!   walker met on a junction starts its battle (`map_objects`).

use sma4_assets::MapNode;

use crate::input::Buttons;
use crate::map_objects::{self, MapObjects};

/// Frames of the map intro (map sub-state 0, the "WORLD n" panel) before Mario can move:
/// sub-state 0 for 182 frames, then 1 for one frame.
pub const INTRO_FRAMES: u16 = 183;
/// The world-start intro (map sub-state 0, 0x0800EB0C), by frame: the panel for 0x80
/// frames (0x0800E88C, timer 0x03003850), on the last of them its window starts closing
/// (0x0802F718: half width 0x03003B6A 0x3C, half height 0x03003B64 0x1C, -8 a frame);
/// then the stars (0x08030738, `map_stars`): set up on frame 135, drawn from 136. Mario
/// is not drawn until they have closed on him (0x0800EA10).
const PANEL_CLOSE: u16 = 127;
const STARS_INIT: u16 = 135;
const INTRO_WINDOW: (u8, u8) = (0x3C, 0x1C);
const WINDOW_STEP: u8 = 8;
/// Walk counter for one move (16 frames at 2 px), -2 per frame.
const WALK: u8 = 0x20;
const STEP: i32 = 2;
/// Up and down stop at these map y values.
const TOP: u8 = 0x20;
const BOTTOM: u8 = 0xA0;
/// Single-direction key masks in the order the ROM searches them (0x0813A59A) and their
/// opposites (0x0813A59E).
const DIRS: [u8; 4] = [0x10, 0x20, 0x80, 0x40];
const OPPOSITE: [u8; 4] = [0x20, 0x10, 0x40, 0x80];
/// Path tiles per direction (0x0813A5C8[dir]: right, left, up, down).
const PATHS: [[u8; 9]; 4] = [
    [0x45, 0xB2, 0xB3, 0xAC, 0xB7, 0xB8, 0xDA, 0xB9, 0xE6],
    [0x45, 0xB2, 0xB3, 0xAC, 0xB7, 0xB8, 0xDA, 0xB9, 0xE6],
    [0x46, 0x46, 0x46, 0xB1, 0xAA, 0xAB, 0xB0, 0xDB, 0xBA],
    [0x46, 0x46, 0x46, 0xB1, 0xAA, 0xAB, 0xB0, 0xDB, 0xBA],
];
/// Drawbridges (0x0813A5D8): passable only while 0x030038C4 matches 0x0813A5DC.
const BRIDGES: [u8; 4] = [0xB2, 0xB2, 0xB1, 0xB1];
const BRIDGE_STATE: [u8; 4] = [0, 0, 1, 1];
/// Neighbour cell per direction (0x08010D18 offsets 0x0813A652 / 0x0813A65A).
const NEIGHBOUR: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, -1), (0, 1)];
/// Tiles A always enters (0x0813A616), besides those at or above the thresholds.
const ENTER: [u8; 11] = [0x50, 0xE8, 0xBC, 0xE0, 0xC9, 0x5F, 0xDF, 0x66, 0xBD, 0xE6, 0xCC];
/// Iris-out (0x0800E1C4 / 0x0802F44C): radius 0x03003B64 from 0xFF, -6 per frame; the
/// level loads once it is below 0 (a few frames later, sub-states 0x14 / 0x15).
const IRIS_START: i16 = 0xFF;
const IRIS_STEP: i16 = 6;
/// Completion (0x08009428): tiles that become a "done" tile 0x08135AFF[kind] (Mario):
/// panels 0x00, Toad houses 0x40, the spade 0xC0. Spots in 0x08135AFA are always done
/// tiles; otherwise those at or above the thresholds 0x0814D934 except 0x67 / 0xEB.
const DONE_ALWAYS: [u8; 5] = [0x50, 0xE8, 0xE6, 0xBD, 0xE0];
const DONE_THRESHOLDS: [u8; 4] = [0x03, 0x67, 0xBF, 0xE9];
const DONE_PANEL: u8 = 0x00;
const DONE_HOUSE: u8 = 0x40;
const DONE_SPADE: u8 = 0xC0;
/// Other cleared tiles (0x08135AEA -> 0x08135AF2): locks and rocks open into paths, the
/// fortress becomes rubble, the bridge appears.
const CHANGES: [(u8, u8); 8] =
    [(0x51, 0x45), (0x52, 0x46), (0x54, 0x46), (0x67, 0x60), (0xEB, 0xE3), (0xE4, 0xDA), (0x56, 0x45), (0x9D, 0xB3)];
/// Clearing a level of a tileset below this (other than a Toad house) moves the
/// checkpoint there (0x080270B8).
const CHECKPOINT_TILESETS: u8 = 0x0F;
const TOAD_HOUSE_TILESET: u8 = 7;
/// Flight back to the checkpoint after a game over (0x080131EC): 0x20 frames, the
/// distance / 32 each frame in 8.8 fixed point.
pub const FLY_FRAMES: u8 = 0x20;

/// How Mario comes back to the map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arrival {
    /// Straight to walking (a cleared level).
    Walk,
    /// The "WORLD n" panel, Mario on his spot (after a lost life: map sub-state 2).
    Panel,
    /// A world (re)start: the panel, then the stars bring Mario in (map sub-state 0: a new
    /// game, CONTINUE).
    World,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapPhase {
    /// The "WORLD n" panel (map sub-state 0).
    Intro(u16),
    /// Waiting for input or walking (sub-state 0x0E).
    Idle,
    /// The iris closing on Mario before a level (sub-states 0x12-0x14); the radius.
    Entering(i16),
    /// After a lost life, Mario going back to the checkpoint (sub-state 3, 0x0800ECFC): the
    /// frame, 1 to `RETURN_FRAMES`.
    Return(u8),
    /// Then the map objects' move (sub-state 0x0C); whether it has started.
    ObjectsMove(bool),
}

/// Sub-state 3: 0x20 frames, the distance split as the game-over flight.
pub const RETURN_FRAMES: u8 = 0x20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldMap {
    /// Map tiles as built from the ROM, `cols` x `rows`.
    base: Vec<u8>,
    pub cols: usize,
    pub rows: usize,
    thresholds: [u8; 4],
    pub nodes: Vec<MapNode>,
    start: (u8, u16),
    /// Where a game over sends Mario back to (0x030037B1 y, B3 screen, B5 x).
    pub checkpoint: (u8, u16),
    /// Completion bits a game over keeps, per column (0x0813C12E).
    game_over_keep: Vec<u8>,
    /// Where the flight back to the checkpoint started.
    fly_from: (u8, u16),
    /// Completion bits per column (0x02001420[world * 0x80 + column]).
    cleared: Vec<u8>,
    /// Tiles with the completions applied.
    grid: Vec<u8>,
    /// Map position (+0 y, +2/+4 x).
    pub y: u8,
    pub x: u16,
    walk: u8,
    dir: u8,
    last_dir: u8,
    cooldown: u8,
    pub phase: MapPhase,
    /// Tile animation (0x0800ADFC): frame 0x03003B5D and its timer 0x03003DAE.
    pub anim_frame: u8,
    anim_timer: i8,
    anim_ticks: [u8; 4],
    /// The spot being entered.
    target: Option<MapNode>,
    /// Screen position of map position (0, 0) with Mario's sprite offsets: his lower 16x16
    /// is at (x, y) + this (0x0800C45C: x - scroll, y - 0x10 - scroll).
    pub sprite_origin: (i32, i32),
    /// The intro is a world start (stars), and its frame (0-based, map sub-state 0).
    world_intro: bool,
    intro_frame: u16,
    pub stars: Option<crate::map_stars::MapStars>,
    /// The map objects (Hammer Bros).
    pub objects: MapObjects,
    /// This frame's random bytes (0x03003879..) and frame counter (0x03003AE8), set by the
    /// game before each step.
    pub rand: [u8; 18],
    pub frame: u32,
    /// The map object Mario met (its slot and item), whose battle is being played.
    pub meeting: Option<(usize, u8)>,
    /// Where the way back to the checkpoint started.
    return_from: (u8, u16),
}

impl WorldMap {
    pub fn new(
        grid: Vec<u8>,
        (cols, rows): (usize, usize),
        start: (u8, u16),
        thresholds: [u8; 4],
        nodes: Vec<MapNode>,
        anim_ticks: [u8; 4],
    ) -> Self {
        let mut map = Self {
            base: grid.clone(),
            cols,
            rows,
            thresholds,
            nodes,
            start,
            checkpoint: start,
            game_over_keep: vec![0xFF; cols],
            fly_from: start,
            cleared: vec![0; cols],
            grid,
            y: start.0,
            x: start.1,
            walk: 0,
            dir: 0,
            last_dir: 0,
            cooldown: 0,
            phase: MapPhase::Intro(INTRO_FRAMES),
            sprite_origin: (0, -0x19),
            world_intro: true,
            intro_frame: 0,
            stars: None,
            anim_frame: 0,
            anim_timer: anim_ticks[0] as i8 - 1,
            anim_ticks,
            target: None,
            objects: MapObjects::default(),
            rand: [0; 18],
            frame: 0,
            meeting: None,
            return_from: start,
        };
        map.rebuild();
        map
    }

    /// Where the map's cell (0, 0) is on screen (`WorldMapAsset::origin`); Mario's sprite is
    /// 0x10 above his map y.
    pub fn with_origin(mut self, origin: (i32, i32)) -> Self {
        self.sprite_origin = (origin.0, origin.1 - 0x10);
        self
    }

    /// The world's map objects (`sma4_assets::MapObjectsAsset`).
    pub fn with_objects(mut self, spawns: &[sma4_assets::MapObjectSpawn]) -> Self {
        self.objects = MapObjects::new(spawns);
        self
    }

    /// The completion bits a game over keeps, per column (`WorldMapAsset::game_over_keep`).
    pub fn with_game_over_keep(mut self, keep: Vec<u8>) -> Self {
        self.game_over_keep = keep;
        self.game_over_keep.resize(self.cols, 0xFF);
        self
    }

    /// A new game: Mario back at START, nothing cleared.
    pub fn reset(&mut self) {
        self.cleared.iter_mut().for_each(|c| *c = 0);
        self.rebuild();
        (self.y, self.x) = self.start;
        self.checkpoint = self.start;
        self.last_dir = 0;
        self.objects.reset();
        self.show(Arrival::World);
    }

    /// Game over (0x08012458): the Toad houses and spade panels are no longer done (their
    /// bits go now; the tiles change when the map is set up again after CONTINUE), and the
    /// flight back starts from where Mario entered the last level.
    pub fn game_over(&mut self) {
        for (c, keep) in self.cleared.iter_mut().zip(&self.game_over_keep) {
            *c &= keep;
        }
        self.walk = 0;
        self.target = None;
        self.fly_from = (self.y, self.x);
    }

    /// Frame `k` (1 to `FLY_FRAMES`) of the flight to the checkpoint (0x080131EC): the
    /// distance is split into 32 steps in 8.8 fixed point (+0x19/+0x1A for y, +0x1D..+0x1F
    /// for x), added (or taken away, with borrow) once per frame.
    pub fn fly(&mut self, k: u8) {
        let along = |from: i32, to: i32| ((from << 8) + (to - from) * 8 * k as i32) >> 8;
        self.y = along(self.fly_from.0 as i32, self.checkpoint.0 as i32) as u8;
        self.x = along(self.fly_from.1 as i32, self.checkpoint.1 as i32) as u16;
    }

    /// After the flight (0x08012BA8): the map is set up again with the game-over bits.
    pub fn continue_game(&mut self) {
        (self.y, self.x) = self.checkpoint;
        self.rebuild();
        self.show(Arrival::World);
    }

    /// Back on the map.
    pub fn show(&mut self, arrival: Arrival) {
        self.walk = 0;
        self.target = None;
        self.meeting = None;
        self.return_from = (self.y, self.x);
        // 0x0800D5B4: the objects on their cells, facing at random.
        self.objects.set_up(&self.rand);
        self.phase = if arrival == Arrival::Walk { MapPhase::Idle } else { MapPhase::Intro(INTRO_FRAMES) };
        self.world_intro = arrival == Arrival::World;
        self.intro_frame = 0;
        self.stars = None;
    }

    /// Mario's lower 16x16 map sprite on screen.
    pub fn sprite_pos(&self) -> (i32, i32) {
        (self.x as i32 + self.sprite_origin.0, self.y as i32 + self.sprite_origin.1)
    }

    /// One frame of the intro (map sub-state 0 / 2).
    fn step_intro(&mut self) {
        let f = self.intro_frame;
        self.intro_frame += 1;
        if !self.world_intro {
            return;
        }
        if f == STARS_INIT {
            self.stars = Some(crate::map_stars::MapStars::new(self.sprite_pos()));
        } else if let Some(stars) = &mut self.stars {
            stars.step();
        }
    }

    /// The panel's window (x, y, w, h on screen), or `None` once it has closed. The HDMA
    /// table built during a frame shows the next one.
    pub fn panel_window(&self) -> Option<(i32, i32, i32, i32)> {
        if !matches!(self.phase, MapPhase::Intro(_)) {
            return None;
        }
        let (w, h) = if self.world_intro {
            // Frames of closing up to the one before this.
            let k = self.intro_frame.saturating_sub(PANEL_CLOSE + 1).min(0xFF) as u8;
            let k = k.min(INTRO_WINDOW.0 / WINDOW_STEP + 1) * WINDOW_STEP;
            (INTRO_WINDOW.0.saturating_sub(k), INTRO_WINDOW.1.saturating_sub(k))
        } else {
            INTRO_WINDOW
        };
        let (w, h) = (w as i32, h as i32);
        use crate::screens::{MENU_WINDOW_X, MENU_WINDOW_Y};
        (w > 0 && h > 0).then_some((MENU_WINDOW_X - w, MENU_WINDOW_Y - h - 1, 2 * w, 2 * h))
    }

    /// Mario is drawn only once the stars have brought him in.
    pub fn mario_hidden(&self) -> bool {
        matches!(self.phase, MapPhase::Intro(_)) && self.world_intro && !self.stars.as_ref().is_some_and(|s| s.done)
    }

    fn cell(&self, y: i32, x: i32) -> (i32, i32) {
        ((x >> 4), ((y - 0x10) >> 4))
    }

    /// Tile at map position (y, x); outside the map counts as ground (0x02).
    pub fn tile_at(&self, y: i32, x: i32) -> u8 {
        let (c, r) = self.cell(y, x);
        self.tile(c, r)
    }

    pub fn tile(&self, col: i32, row: i32) -> u8 {
        if col < 0 || row < 0 || col as usize >= self.cols || row as usize >= self.rows {
            return 0x02;
        }
        self.grid[row as usize * self.cols + col as usize]
    }

    fn enterable(&self, tile: u8) -> bool {
        tile >= self.thresholds[(tile >> 6) as usize]
    }

    /// Re-applies the completion bits to the ROM tiles (0x08009428).
    fn rebuild(&mut self) {
        self.grid = self.base.clone();
        for col in 0..self.cols {
            for bit in 0..8 {
                if self.cleared[col] & (0x80 >> bit) != 0 {
                    self.complete(col, bit + 1, bit == 7);
                }
            }
        }
    }

    fn complete(&mut self, col: usize, mut row: usize, last_bit: bool) {
        while row < self.rows {
            let i = row * self.cols + col;
            let tile = self.grid[i];
            let done = DONE_ALWAYS.contains(&tile)
                || (tile != 0x67 && tile != 0xEB && tile >= DONE_THRESHOLDS[(tile >> 6) as usize]);
            if done {
                self.grid[i] = match tile {
                    0xE8 => DONE_SPADE,
                    0x50 | 0xE0 => DONE_HOUSE,
                    _ => DONE_PANEL,
                };
                return;
            }
            if let Some(&(_, to)) = CHANGES.iter().find(|&&(from, _)| from == tile) {
                self.grid[i] = to;
                return;
            }
            // Bit 0 covers the last two rows: look one row further down.
            if !last_bit || row >= 9 {
                return;
            }
            row += 1;
        }
    }

    /// Marks the spot at cell (col, row) cleared, as if its level had been beaten.
    pub fn clear_cell(&mut self, col: usize, row: usize) {
        if col < self.cols && (1..self.rows).contains(&row) {
            self.cleared[col] |= 0x80 >> (row - 1).min(7);
            self.rebuild();
        }
    }

    /// Marks the spot under Mario cleared (its bit in 0x02001420) and redraws it; a
    /// level there becomes the checkpoint (0x080270B8).
    pub fn clear_here(&mut self) {
        if self.node_here().is_some_and(|n| n.tileset < CHECKPOINT_TILESETS && n.tileset != TOAD_HOUSE_TILESET) {
            self.checkpoint = (self.y, self.x);
        }
        let (c, r) = self.cell(self.y as i32, self.x as i32);
        if c < 0 || r < 1 || c as usize >= self.cols {
            return;
        }
        self.cleared[c as usize] |= 0x80 >> (r - 1).min(7);
        self.rebuild();
    }

    /// The level-table entry for Mario's position (0x08009A3C).
    pub fn node_here(&self) -> Option<MapNode> {
        self.nodes.iter().copied().find(|n| n.y == self.y & 0xF0 && n.x == self.x & 0xFFF0)
    }

    /// 0x08010E20: starts a move if the key is one direction and the neighbouring cell is a
    /// path for it.
    fn try_move(&mut self, held: Buttons) -> bool {
        if self.cooldown != 0 && self.cooldown != 0x18 {
            return false;
        }
        let keys = (held.bits() & 0xF0) as u8;
        if !DIRS.contains(&keys) {
            return false;
        }
        let dir = keys.trailing_zeros() as usize - 4;
        let (dx, dy) = NEIGHBOUR[dir];
        let (c, r) = self.cell(self.y as i32, self.x as i32);
        let next = self.tile(c + dx, r + dy);
        if !PATHS[dir].contains(&next) || (next == BRIDGES[dir] && BRIDGE_STATE[dir] != 0) {
            return false;
        }
        self.walk = WALK;
        self.dir = keys >> 4;
        self.last_dir = keys;
        true
    }

    fn walk_step(&mut self) {
        self.walk -= 2;
        match self.dir {
            1 => self.x = self.x.wrapping_add(STEP as u16),
            2 => self.x = self.x.wrapping_sub(STEP as u16),
            4 => self.y = self.y.wrapping_sub(STEP as u8),
            _ => self.y = self.y.wrapping_add(STEP as u8),
        }
    }

    /// One frame. Returns the spot to enter once the iris has closed.
    pub fn step(&mut self, held: Buttons, pressed: Buttons) -> Option<MapNode> {
        // 0x0800ADFC: tile animation.
        self.anim_timer -= 1;
        if self.anim_timer < 0 {
            self.anim_frame = (self.anim_frame + 1) & 3;
            self.anim_timer = self.anim_ticks[self.anim_frame as usize] as i8 - 1;
        }
        match self.phase {
            MapPhase::Intro(t) => {
                // The objects sway on the world-start panel (sub-state 0), not on the one
                // after a lost life (2).
                if self.world_intro {
                    self.objects.step_idle(self.frame);
                }
                self.step_intro();
                self.phase = if t > 1 {
                    MapPhase::Intro(t - 1)
                } else if self.world_intro {
                    MapPhase::Idle
                } else {
                    MapPhase::Return(0)
                };
                None
            }
            MapPhase::Return(k) => {
                // 0x0800ECFC: the distance to the checkpoint in 32 steps of 8.8 fixed point.
                let k = k + 1;
                let along = |from: i32, to: i32| ((from << 8) + (to - from) * 8 * k as i32) >> 8;
                self.y = along(self.return_from.0 as i32, self.checkpoint.0 as i32) as u8;
                self.x = along(self.return_from.1 as i32, self.checkpoint.1 as i32) as u16;
                self.phase = if k < RETURN_FRAMES { MapPhase::Return(k) } else { MapPhase::ObjectsMove(false) };
                None
            }
            MapPhase::ObjectsMove(started) => {
                let mut objects = std::mem::take(&mut self.objects);
                if !started {
                    objects.start_move();
                }
                let done = objects.step_move(&self.rand, self);
                self.objects = objects;
                self.phase = if done { MapPhase::Idle } else { MapPhase::ObjectsMove(true) };
                None
            }
            MapPhase::Entering(r) => {
                let r = r - IRIS_STEP;
                if r < 0 {
                    self.phase = MapPhase::Idle;
                    return self.target.take();
                }
                self.phase = MapPhase::Entering(r);
                None
            }
            MapPhase::Idle => {
                self.idle(held, pressed);
                self.objects.step_idle(self.frame);
                // 0x08031DDC: Mario standing on a walker's junction starts its battle at
                // once (map sub-state 0x16, no iris).
                if self.walk == 0
                    && self.target.is_none()
                    && let Some(slot) = self.objects.meeting(self.y, self.x, self.tile_at(self.y as i32, self.x as i32))
                    && let Some(node) = self.node_here()
                {
                    self.meeting = Some((slot, self.objects.slots[slot].item));
                    return Some(node);
                }
                None
            }
        }
    }

    /// The battle with the map object Mario met was won (its chest opened): the object is
    /// gone (0x0800F8C8) and, as for a cleared level, the spot is the checkpoint
    /// (0x080270B8; recorded in mGBA).
    pub fn battle_won(&mut self) {
        if let Some((slot, _)) = self.meeting.take() {
            self.objects.remove(slot);
            self.checkpoint = (self.y, self.x);
        }
    }

    /// The map objects as drawn.
    pub fn object_sprites(&self) -> Vec<map_objects::MapObjectSprite> {
        self.objects.sprites(self.frame, matches!(self.phase, MapPhase::ObjectsMove(_)))
    }

    /// Map sub-state 0x0E (0x0801112C), with the cooldown update of 0x0800D5B4 before it.
    fn idle(&mut self, held: Buttons, pressed: Buttons) {
        if self.walk == 0 {
            if self.cooldown != 0x18 {
                self.cooldown = self.cooldown.wrapping_add(1);
            }
            if pressed.bits() & 0x3FF == 0 {
                self.cooldown = 0xFF;
            }
            let blocked = (held.contains(Buttons::UP) && self.y == TOP) || (held.contains(Buttons::DOWN) && self.y == BOTTOM);
            if blocked {
                return;
            }
            let tile = self.tile_at(self.y as i32, self.x as i32);
            let keys = (held.bits() & 0xF0) as u8;
            if !self.enterable(tile) || keys == 0 {
                self.try_move(held);
            } else {
                // On a spot not cleared yet: only back the way Mario came.
                let i = (1..4).rev().find(|&i| DIRS[i] == keys).unwrap_or(0);
                if self.last_dir == OPPOSITE[i] {
                    self.try_move(held);
                } else {
                    let last = self.last_dir;
                    if self.try_move(held) {
                        self.walk = 0;
                    }
                    self.last_dir = last;
                }
            }
            if self.walk == 0 {
                if pressed.contains(Buttons::A) && (ENTER.contains(&tile) || self.enterable(tile)) {
                    self.target = self.node_here();
                    if self.target.is_some() {
                        self.phase = MapPhase::Entering(IRIS_START);
                    }
                }
                return;
            }
        }
        self.walk_step();
    }

    /// Whether Mario is walking (for the sprite).
    pub fn walking(&self) -> bool {
        self.walk != 0
    }
}

impl map_objects::Grid for WorldMap {
    fn tile_at(&self, y: i32, x: i32) -> u8 {
        WorldMap::tile_at(self, y, x)
    }
    fn enterable(&self, tile: u8) -> bool {
        WorldMap::enterable(self, tile)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// World 1's grid (rows 0-11), as the map init builds it.
    pub(crate) const W1: [&str; 12] = [
        "b4 b4 b4 b4 42 42 42 42 42 42 42 42 42 b4 b4 b4",
        "b4 b4 b4 b4 03 45 47 45 04 45 05 45 47 b4 b4 b4",
        "b4 b4 b4 b4 46 b4 b4 b4 46 b4 b4 b4 46 b4 b4 b4",
        "49 49 e5 45 4a b4 b4 b4 48 45 06 45 50 b4 b4 b4",
        "b4 b4 b4 b4 54 b4 b4 b4 46 b4 b4 b4 b4 b4 b4 b4",
        "b4 b4 b4 b4 48 45 67 45 e8 b4 b4 99 a2 9a b4 b4",
        "b4 b4 42 42 46 42 b4 b4 b4 99 a2 a7 c8 9d b4 b4",
        "42 42 42 53 48 53 50 45 47 b3 47 45 c9 9d b4 b4",
        "42 42 53 53 46 53 53 42 46 9d 42 42 42 9d b4 b4",
        "42 42 53 53 07 45 47 45 08 a6 a2 a2 a2 a7 b4 b4",
        "42 42 42 53 53 53 42 42 42 42 42 42 b4 42 42 b4",
        "42 42 42 42 42 42 42 42 42 42 42 42 42 42 42 42",
    ];

    pub(crate) fn world1() -> WorldMap {
        let grid = W1.iter().flat_map(|r| r.split(' ').map(|t| u8::from_str_radix(t, 16).unwrap())).collect();
        let node = |y, x, level| MapNode { y, x, tileset: 1, level, enemies: 0 };
        let nodes = vec![node(0x20, 0x40, 0x22), node(0x20, 0x80, 0x4B), node(0x40, 0xC0, 0x102)];
        // World 1's game-over mask (0x0813C12E): levels 1-1 to 1-6, the fortress and its lock.
        let mut keep = vec![0; 16];
        (keep[4], keep[6], keep[8], keep[10]) = (0x91, 0x08, 0x81, 0xA0);
        let mut m = WorldMap::new(grid, (16, 12), (0x40, 0x20), [0x03, 0x67, 0xBF, 0xE9], nodes, [17; 4])
            .with_game_over_keep(keep);
        m.show(Arrival::Walk);
        m
    }

    /// Runs a script like the probe's: (keys, frames), keys pressed on the first frame.
    fn run(m: &mut WorldMap, script: &[(Buttons, u32)]) -> Option<MapNode> {
        let mut held = Buttons::empty();
        let mut entered = None;
        for &(keys, n) in script {
            for _ in 0..n {
                let pressed = keys & !held;
                held = keys;
                entered = entered.or(m.step(held, pressed));
            }
        }
        entered
    }

    #[test]
    fn a_press_walks_two_cells_in_16_frames() {
        let mut m = world1();
        run(&mut m, &[(Buttons::empty(), 1), (Buttons::RIGHT, 1)]);
        assert_eq!((m.x, m.walk), (0x22, 0x1E), "first step on the press frame");
        run(&mut m, &[(Buttons::RIGHT, 15)]);
        assert_eq!((m.y, m.x, m.walk), (0x40, 0x40, 0));
        // Ground to the right: no move.
        run(&mut m, &[(Buttons::empty(), 1), (Buttons::RIGHT, 20)]);
        assert_eq!(m.x, 0x40);
    }

    #[test]
    fn holding_the_pad_stops_at_the_next_spot() {
        let mut m = world1();
        m.clear_here();
        (m.y, m.x) = (0x20, 0x40);
        m.clear_here();
        run(&mut m, &[(Buttons::empty(), 1), (Buttons::RIGHT, 80)]);
        assert_eq!(m.x, 0x60, "one move per press");
    }

    #[test]
    fn on_an_uncleared_panel_only_back_the_way_he_came() {
        let mut m = world1();
        run(&mut m, &[(Buttons::empty(), 1), (Buttons::RIGHT, 20), (Buttons::empty(), 1), (Buttons::UP, 20)]);
        assert_eq!((m.y, m.x), (0x20, 0x40), "on panel 1");
        run(&mut m, &[(Buttons::empty(), 1), (Buttons::RIGHT, 20)]);
        assert_eq!((m.y, m.x), (0x20, 0x40), "can't pass it");
        run(&mut m, &[(Buttons::empty(), 1), (Buttons::DOWN, 20)]);
        assert_eq!((m.y, m.x), (0x40, 0x40), "back down");
    }

    #[test]
    fn cleared_panels_turn_into_done_tiles_and_open_the_way() {
        let mut m = world1();
        (m.y, m.x) = (0x20, 0x40);
        m.clear_here();
        assert_eq!(m.tile_at(0x20, 0x40), DONE_PANEL);
        run(&mut m, &[(Buttons::empty(), 1), (Buttons::RIGHT, 20)]);
        assert_eq!(m.x, 0x60);
        // Panel 5 on the last row shares bit 0 with row 8.
        (m.y, m.x) = (0xA0, 0x40);
        m.clear_here();
        assert_eq!(m.tile(4, 9), DONE_PANEL);
        assert_eq!(m.tile(4, 8), 0x46);
        // Toad house and fortress.
        (m.y, m.x) = (0x40, 0xC0);
        m.clear_here();
        assert_eq!(m.tile(12, 3), DONE_HOUSE);
        (m.y, m.x) = (0x60, 0x60);
        m.clear_here();
        assert_eq!(m.tile(6, 5), 0x60);
    }

    #[test]
    fn a_on_a_panel_closes_the_iris_then_enters_its_level() {
        let mut m = world1();
        let entered = run(&mut m, &[
            (Buttons::empty(), 1),
            (Buttons::RIGHT, 20),
            (Buttons::empty(), 1),
            (Buttons::UP, 20),
            (Buttons::A, 1),
        ]);
        assert!(entered.is_none());
        assert_eq!(m.phase, MapPhase::Entering(IRIS_START));
        let mut frames = 0;
        let node = loop {
            frames += 1;
            if let Some(n) = m.step(Buttons::empty(), Buttons::empty()) {
                break n;
            }
        };
        assert_eq!(node.level, 0x22);
        assert_eq!(frames, 43, "0xFF down by 6 until below 0");
        // Not on a path tile.
        let mut m = world1();
        (m.y, m.x) = (0x40, 0x40);
        run(&mut m, &[(Buttons::A, 1)]);
        assert_eq!(m.phase, MapPhase::Idle);
    }

    #[test]
    fn intro_lasts_183_frames() {
        let mut m = world1();
        m.show(Arrival::Panel);
        run(&mut m, &[(Buttons::empty(), 182)]);
        assert!(matches!(m.phase, MapPhase::Intro(1)));
        run(&mut m, &[(Buttons::empty(), 1)]);
        assert_eq!(m.phase, MapPhase::Return(0));
    }

    #[test]
    fn walking_onto_the_hammer_bro_starts_its_battle() {
        // As set up in mGBA: the Hammer Bro (slot 2, item 9) moved onto the junction right
        // of START; walking right onto it loads that cell's level (0x4E) at once.
        let mut m = world1();
        m.nodes.push(MapNode { y: 0x40, x: 0x40, tileset: 3, level: 0x4E, enemies: 0x61 });
        m = m.with_objects(&[sma4_assets::MapObjectSpawn { slot: 2, kind: map_objects::HAMMER_BRO, y: 0x40, x: 0x40, item: 9 }]);
        m.show(Arrival::Walk);
        let node = run(&mut m, &[(Buttons::empty(), 1), (Buttons::RIGHT, 1), (Buttons::empty(), 20)]).expect("a battle");
        assert_eq!((node.level, node.enemies), (0x4E, 0x61));
        assert_eq!(m.meeting, Some((2, 9)));
        assert_eq!(m.phase, MapPhase::Idle, "no iris");
        // Won: the Bro is gone and the junction is the checkpoint.
        m.battle_won();
        assert_eq!(m.objects.slots[2].kind, 0);
        assert_eq!(m.checkpoint, (0x40, 0x40));
    }

    #[test]
    fn after_a_lost_life_mario_goes_back_to_the_checkpoint() {
        // Recorded in mGBA: dying in 1-1 (0x20, 0x40) slides Mario back to START (0x40,
        // 0x20) in 0x20 frames (sub-state 3), then the objects move (0x0C).
        let mut m = world1();
        (m.y, m.x) = (0x20, 0x40);
        m.show(Arrival::Panel);
        run(&mut m, &[(Buttons::empty(), INTRO_FRAMES as u32)]);
        run(&mut m, &[(Buttons::empty(), 1)]);
        assert_eq!((m.y, m.x), (0x21, 0x3F));
        run(&mut m, &[(Buttons::empty(), RETURN_FRAMES as u32 - 1)]);
        assert_eq!((m.y, m.x), (0x40, 0x20));
        assert_eq!(m.phase, MapPhase::ObjectsMove(false));
        run(&mut m, &[(Buttons::empty(), 1)]);
        assert_eq!(m.phase, MapPhase::Idle, "no objects: the move is over at once");
    }

    #[test]
    fn tiles_animate_every_17_frames() {
        let mut m = world1();
        let mut seen = Vec::new();
        for _ in 0..70 {
            m.step(Buttons::empty(), Buttons::empty());
            seen.push(m.anim_frame);
        }
        assert_eq!(seen[15], 0);
        assert_eq!(seen[16], 1);
        assert_eq!(seen[33], 2);
    }

    #[test]
    fn clearing_a_level_moves_the_checkpoint_and_game_over_flies_back_to_it() {
        let mut m = world1();
        assert_eq!(m.checkpoint, (0x40, 0x20), "START");
        (m.y, m.x) = (0x20, 0x40);
        m.clear_here();
        assert_eq!(m.checkpoint, (0x20, 0x40), "1-1 cleared");
        m.clear_cell(12, 3);
        m.clear_cell(8, 5);
        (m.y, m.x) = (0x20, 0x80);
        m.game_over();
        assert_eq!(m.tile(12, 3), 0x40, "the old tiles stay until the map is set up again");
        let path: Vec<(u8, u16)> = (1..=FLY_FRAMES).map(|k| {
            m.fly(k);
            (m.y, m.x)
        }).collect();
        assert_eq!(path[0], (0x20, 0x7E), "64 px in 32 steps of 2");
        assert_eq!(path[31], (0x20, 0x40));
        m.continue_game();
        assert_eq!((m.y, m.x), (0x20, 0x40));
        assert_eq!(m.tile(4, 1), 0x00, "1-1 stays cleared");
        assert_eq!((m.tile(12, 3), m.tile(8, 5)), (0x50, 0xE8), "Toad house and spade come back");
        m.reset();
        assert_eq!(m.checkpoint, (0x40, 0x20));
    }

    #[test]
    fn the_flight_rounds_like_the_rom() {
        let mut m = world1();
        (m.y, m.x) = (0xA0, 0xC0);
        m.game_over();
        m.fly(1);
        // y 0xA0 -> 0x40: -0x60 = 3 px a frame; x 0xC0 -> 0x20: -0xA0 = 5 px a frame.
        assert_eq!((m.y, m.x), (0x9D, 0xBB));
        m.checkpoint = (0x40, 0x23);
        m.fly(1);
        assert_eq!(m.x, 0xBB, "-0x9D / 32 = -4.9: the borrow rounds down");
        m.fly(FLY_FRAMES);
        assert_eq!((m.y, m.x), (0x40, 0x23));
    }

    #[test]
    fn a_world_start_closes_the_panel_then_the_stars_bring_mario_in() {
        let mut m = world1().with_origin((0, -9));
        m.show(Arrival::World);
        assert_eq!(m.sprite_pos(), (0x20, 0x27), "the ROM's OAM for Mario on START");
        let full = m.panel_window();
        let mut first_star = None;
        let mut mario_at = None;
        for f in 0..INTRO_FRAMES {
            m.step(Buttons::empty(), Buttons::empty());
            if f < PANEL_CLOSE {
                assert_eq!(m.panel_window(), full, "frame {f}");
            }
            if f == PANEL_CLOSE + 7 {
                assert_eq!(m.panel_window(), None, "closed");
            }
            if first_star.is_none() && m.stars.as_ref().is_some_and(|s| s.stars().next().is_some()) {
                first_star = Some(f);
            }
            if mario_at.is_none() && !m.mario_hidden() {
                mario_at = Some(f);
            }
        }
        assert_eq!((first_star, mario_at), (Some(136), Some(181)));
        assert_eq!(m.phase, MapPhase::Idle);

        // After a lost life: the panel only, Mario on his spot.
        m.show(Arrival::Panel);
        assert!(!m.mario_hidden());
        for _ in 0..INTRO_FRAMES {
            m.step(Buttons::empty(), Buttons::empty());
            assert!(m.stars.is_none());
        }
    }
}
