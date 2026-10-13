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

use sma4_assets::MapNode;

use crate::input::Buttons;

/// Frames of the map intro (map sub-state 0, the "WORLD n" panel) before Mario can move:
/// sub-state 0 for 182 frames, then 1 for one frame.
pub const INTRO_FRAMES: u16 = 183;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapPhase {
    /// The "WORLD n" panel (map sub-state 0).
    Intro(u16),
    /// Waiting for input or walking (sub-state 0x0E).
    Idle,
    /// The iris closing on Mario before a level (sub-states 0x12-0x14); the radius.
    Entering(i16),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldMap {
    /// Map tiles as built from the ROM, `cols` x `rows`.
    base: Vec<u8>,
    pub cols: usize,
    pub rows: usize,
    thresholds: [u8; 4],
    pub nodes: Vec<MapNode>,
    start: (u8, u16),
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
            cleared: vec![0; cols],
            grid,
            y: start.0,
            x: start.1,
            walk: 0,
            dir: 0,
            last_dir: 0,
            cooldown: 0,
            phase: MapPhase::Intro(INTRO_FRAMES),
            anim_frame: 0,
            anim_timer: anim_ticks[0] as i8 - 1,
            anim_ticks,
            target: None,
        };
        map.rebuild();
        map
    }

    /// A new game: Mario back at START, nothing cleared.
    pub fn reset(&mut self) {
        self.cleared.iter_mut().for_each(|c| *c = 0);
        self.rebuild();
        (self.y, self.x) = self.start;
        self.last_dir = 0;
        self.show(true);
    }

    /// Back on the map, with the intro panel (start, after a lost life) or straight to
    /// walking (after a cleared level).
    pub fn show(&mut self, intro: bool) {
        self.walk = 0;
        self.target = None;
        self.phase = if intro { MapPhase::Intro(INTRO_FRAMES) } else { MapPhase::Idle };
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

    /// Marks the spot under Mario cleared (its bit in 0x02001420) and redraws it.
    pub fn clear_here(&mut self) {
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
                self.phase = if t > 1 { MapPhase::Intro(t - 1) } else { MapPhase::Idle };
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
                None
            }
        }
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

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// World 1's grid (rows 0-11), as the map init builds it.
    const W1: [&str; 12] = [
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
        let mut m = WorldMap::new(grid, (16, 12), (0x40, 0x20), [0x03, 0x67, 0xBF, 0xE9], nodes, [17; 4]);
        m.show(false);
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
        m.show(true);
        run(&mut m, &[(Buttons::empty(), 182)]);
        assert!(matches!(m.phase, MapPhase::Intro(1)));
        run(&mut m, &[(Buttons::empty(), 1)]);
        assert_eq!(m.phase, MapPhase::Idle);
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
}
