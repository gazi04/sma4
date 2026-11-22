//! Map objects: the Hammer Bros that walk the world map and start a battle when they meet
//! Mario (see docs/re-notes.md, "Map objects and Hammer Bros").
//!
//! * Slots (14 per player, 0x03002A6E y / 0x03002B4E screen / 0x03002ADE x / 0x03002BBE
//!   kind + player * 14): a new game copies six per world from 0x08139F60.. (0x08039AC8),
//!   each holding the item of 0x08139FE0 (0x03003790, World 1's Hammer Bro: 9, a Starman).
//! * Live position 0x03003D15 (y) / 0x03003D33:0x03003D24 (screen:x), direction 0x03003D42,
//!   counter 0x03003D51 (slot 0's byte doubles as the sway timer). Setting the map up after a
//!   level (0x0800D5B4) puts the live position on the slot's and the direction on
//!   0x03003879[slot] & 3.
//! * Every map frame 0x080329F4 runs each slot's behaviour (0x081426A4[kind]; walkers, kinds
//!   3-6 and 8-11: 0x08031558) and draws it (0x08031DDC), then, on the panel (sub-state 0)
//!   and while idle (0x0E), moves the sway timer on every 4 frames.
//! * Idle, a walker sways: x = its cell + 0x0813A704[(t & 0x1F) ^ (slot & 1) << 4] (kind 5:
//!   0x0813A724), facing (t >> 4 & 1) ^ (slot & 1).
//! * Map sub-state 0x0C (after a lost life, once Mario has gone back to the checkpoint):
//!   the counters start at 0x0813A6E4[kind] (0x20 for walkers); a walker picks a direction
//!   (0x080313AC) whenever its counter is a multiple of 0x20 and walks 1 px a frame
//!   (0x0813A6F8 y / 0x0813A6FC x); a counter run out on another walker's spot walks on
//!   (0x08031190). When every counter is 0 (0x08010938) the slots snap to their cells.
//! * Mario idle on a walker's cell, on a junction tile (0x081426EC), starts its battle
//!   (0x08031DDC): map sub-state 0x16, 0x03003AF7 = kind, 0x0300379D = its item; the level is
//!   the level table's entry for Mario's cell (0x08009A3C).

/// Map object kinds.
pub const HAMMER_BRO: u8 = 3;
/// Slots per player (0x03002BBE + player * 14).
pub const SLOTS: usize = 14;
/// Kinds run by 0x08031558 (0x081426A4).
const WALKERS: [u8; 8] = [3, 4, 5, 6, 8, 9, 10, 11];
/// 0x0813A6E4[kind]: frames of a move.
const MOVE_FRAMES: [u8; 16] = [0, 0, 0, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0, 0, 0x20, 0, 0, 0, 0];
/// 0x08142694[kind]: walkers that meet at the end of a move walk on.
const COLLIDES: [bool; 16] =
    [false, false, false, true, true, true, true, false, true, false, false, false, false, false, false, false];
/// Sway offsets 0x0813A704 (0x0813A724 for kind 5).
const SWAY: [i8; 32] = [
    -8, -7, -6, -5, -4, -3, -2, -1, 0, 1, 2, 3, 4, 5, 6, 7, 8, 7, 6, 5, 4, 3, 2, 1, 0, -1, -2, -3, -4, -5, -6, -7,
];
const SWAY_5: [i8; 32] = [
    -6, -5, -4, -3, -3, -2, -1, -1, 0, 0, 1, 2, 3, 3, 4, 5, 6, 5, 4, 3, 3, 2, 1, 1, 0, -1, -1, -2, -3, -3, -4, -5,
];
/// Per direction (0 right, 1 left, 2 down, 3 up): a frame's step (0x0813A6F8 y, 0x0813A6FC x).
const STEP_Y: [i32; 4] = [0, 0, 1, -1];
const STEP_X: [i32; 4] = [1, -1, 0, 0];
/// 0x080312D4's cell offsets (0x0813A764 y, 0x0813A76C x): directions 0-3 one cell over,
/// 4-7 two.
const LOOK_Y: [i32; 8] = [0, 0, 0x10, -0x10, 0, 0, 0x20, -0x20];
const LOOK_X: [i32; 8] = [0x10, -0x10, 0, 0, 0x20, -0x20, 0, 0];
/// Path tiles a walker may step onto, per direction (0x0813A5C8[dir], as Mario's).
const PATHS: [[u8; 9]; 4] = [
    [0x45, 0xB2, 0xB3, 0xAC, 0xB7, 0xB8, 0xDA, 0xB9, 0xE6],
    [0x45, 0xB2, 0xB3, 0xAC, 0xB7, 0xB8, 0xDA, 0xB9, 0xE6],
    [0x46, 0x46, 0x46, 0xB1, 0xAA, 0xAB, 0xB0, 0xDB, 0xBA],
    [0x46, 0x46, 0x46, 0xB1, 0xAA, 0xAB, 0xB0, 0xDB, 0xBA],
];
/// Spots a walker may not stop on (0x0813A77C, 17): cleared panels, Toad houses, the spade,
/// START...
const NO_STOP: [u8; 17] = [0x40, 0x41, 0x00, 0x01, 0xBB, 0xB4, 0xBC, 0xE8, 0x5F, 0xDF, 0x60, 0x80, 0x81, 0xC0, 0xC1, 0xE3, 0xE5];
/// Spots a walker walks across (0x0813A78D, 4), as it does levels not cleared yet.
const WALK_ACROSS: [u8; 4] = [0x50, 0xE0, 0xC9, 0x66];
/// Tiles Mario must stand on for a walker to meet him (0x081426EC; plane-2 entries left out).
const MEETING_TILES: [u8; 12] = [0x44, 0x47, 0x48, 0x4A, 0xAE, 0xAF, 0xB5, 0xB6, 0xD9, 0xDC, 0xDD, 0xDE];
/// After a move, the sway timer restarts here (0x08010938).
const TIMER_AFTER_MOVE: u8 = 8;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MapObject {
    pub kind: u8,
    /// The item its battle's chest holds (0x03003790[slot]).
    pub item: u8,
    /// Its cell (map y; screen << 8 | x).
    pub y: u8,
    pub x: u16,
    /// Live position, as drawn.
    pub live_y: u8,
    pub live_x: u16,
    /// 0x03003D42: 0 right, 1 left, 2 down, 3 up.
    pub dir: u8,
    /// 0x03003D51.
    count: u8,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MapObjects {
    pub slots: [MapObject; SLOTS],
    /// A new game's objects.
    spawns: [MapObject; SLOTS],
}

/// What the map grid looks like to a walker.
pub trait Grid {
    /// Tile at map position (y, x).
    fn tile_at(&self, y: i32, x: i32) -> u8;
    /// At or above the enterable threshold (0x030037D6[tile >> 6]).
    fn enterable(&self, tile: u8) -> bool;
}

/// A map object as drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapObjectSprite {
    pub kind: u8,
    /// Map position of its 16x16 (as Mario's lower half).
    pub y: i32,
    pub x: i32,
    /// `sma4_assets::map_object_cell` frame.
    pub frame: u32,
    pub flip: bool,
}

fn walker(kind: u8) -> bool {
    WALKERS.contains(&kind)
}

/// The airship waiting on the map (kind 2, behaviour 0x08030844).
pub const AIRSHIP: u8 = 2;

/// Drawn by 0x08031DDC: the walkers and the airship.
fn drawn(kind: u8) -> bool {
    walker(kind) || kind == AIRSHIP
}

impl MapObjects {
    /// A world's objects (`sma4_assets::MapObjectsAsset`).
    pub fn new(spawns: &[sma4_assets::MapObjectSpawn]) -> Self {
        let mut s = [MapObject::default(); SLOTS];
        for o in spawns.iter().filter(|o| (o.slot as usize) < SLOTS) {
            s[o.slot as usize] =
                MapObject { kind: o.kind, item: o.item, y: o.y, x: o.x, live_y: o.y, live_x: o.x, dir: 0, count: 0 };
        }
        Self { slots: s, spawns: s }
    }

    /// A new game (0x08039AC8).
    pub fn reset(&mut self) {
        self.slots = self.spawns;
    }

    /// The sway timer (0x03003D51, slot 0's counter).
    fn timer(&self) -> u8 {
        self.slots[0].count
    }

    /// The map set up again after a level (0x0800D5B4): live positions on the cells,
    /// directions from the slots' random bytes.
    pub fn set_up(&mut self, rand: &[u8; 18]) {
        for (i, o) in self.slots.iter_mut().enumerate() {
            (o.live_y, o.live_x) = (o.y, o.x);
            if o.kind < 0x10 {
                o.dir = rand[i] & 3;
            }
        }
    }

    /// One frame on the panel or idle (0x080329F4): the walkers sway, then the timer moves on
    /// every 4 frames.
    pub fn step_idle(&mut self, frame: u32) {
        self.sway();
        if frame & 3 == 0 {
            self.slots[0].count = self.slots[0].count.wrapping_add(1);
        }
    }

    /// The walkers' sway positions (0x08031558 outside sub-state 0x0C); 0x080329F4 moves
    /// the sway timer only in sub-states 0 and 0x0E.
    pub fn sway(&mut self) {
        let t = self.timer();
        for (i, o) in self.slots.iter_mut().enumerate().rev() {
            if !walker(o.kind) {
                continue;
            }
            // 0x08031558 outside sub-state 0x0C.
            let parity = (i & 1) as u8;
            o.dir = (t >> 4 & 1) ^ parity;
            let idx = ((parity << 4) ^ (t & 0x1F)) as usize;
            let off = if o.kind == 5 { SWAY_5[idx] } else { SWAY[idx] };
            o.live_x = o.x.wrapping_add(off as i16 as u16);
        }
    }

    /// 0x08030844 (map sub-state 0x0E): the airship faces Mario (0x03003D43 = 1 while he is
    /// left of it).
    pub fn face_airship(&mut self, mario_x: u16) {
        for o in self.slots.iter_mut().filter(|o| o.kind == AIRSHIP) {
            o.dir = (mario_x < o.live_x) as u8;
        }
    }

    /// The first frame of map sub-state 0x0C (0x08031A88): every object's counter starts.
    pub fn start_move(&mut self) {
        for o in &mut self.slots {
            if o.kind != 0 {
                o.count = MOVE_FRAMES[o.kind as usize & 0xF];
            }
        }
    }

    /// One frame of sub-state 0x0C (0x08031A88, then 0x08010938). `rand` is 0x03003879..
    /// for this frame. True once every object has stopped (they are then on their cells).
    pub fn step_move(&mut self, rand: &[u8; 18], grid: &impl Grid) -> bool {
        for i in (0..SLOTS).rev() {
            let o = self.slots[i];
            if o.kind == 0 || (o.kind < 0x10 && o.count == 0) || !walker(o.kind) {
                continue;
            }
            // 0x08031558, sub-state 0x0C.
            if o.count & 0x1F == 0 && !self.pick_direction(i, rand[i], grid) {
                continue;
            }
            let o = &mut self.slots[i];
            let d = o.dir as usize & 3;
            o.live_y = (o.live_y as i32 + STEP_Y[d]) as u8;
            o.live_x = (o.live_x as i32 + STEP_X[d]) as u16;
            // 0x08031264: the cell's x follows, the counter runs down.
            o.x = o.live_x;
            o.count -= 1;
            if o.count == 0 {
                for j in (0..SLOTS).rev() {
                    if self.meet(i, j) {
                        break;
                    }
                }
            }
        }
        if self.slots.iter().any(|o| o.count != 0) {
            return false;
        }
        // 0x08010938: on their cells; the sway timer restarts.
        for o in &mut self.slots {
            o.y = o.live_y.wrapping_add(8) & 0xF0;
            o.x = (o.live_x.wrapping_add(8)) & 0xFFF0;
        }
        self.slots[0].count = TIMER_AFTER_MOVE;
        true
    }

    /// 0x08031190: walker `i` stopping where walker `j` stands walks on (and so does `j`
    /// if it can).
    fn meet(&mut self, i: usize, j: usize) -> bool {
        let (a, b) = (self.slots[i], self.slots[j]);
        if i == j || !(1..0x10).contains(&b.kind) || b.count != 0 || (a.live_y, a.live_x) != (b.live_y, b.live_x) {
            return false;
        }
        let (ca, cb) = (COLLIDES[a.kind as usize & 0xF], COLLIDES[b.kind as usize & 0xF]);
        if !ca && !cb {
            return false;
        }
        self.slots[i].count = 0x20;
        if cb {
            self.slots[j].count = 0x20;
        }
        true
    }

    /// 0x080313AC: turns from `start` by one (the way bit 7 of the slot's random byte says)
    /// until the next cell is a path, never straight back but as the fourth try; the spot two
    /// cells on must not be one it may not stop on, and across a level or a few others the
    /// move is doubled. False (and the counter cleared) when it found no way.
    fn pick_direction(&mut self, i: usize, rand: u8, grid: &impl Grid) -> bool {
        let o = self.slots[i];
        let look = |d: usize| grid.tile_at(o.live_y as i32 + LOOK_Y[d], o.live_x as i32 + LOOK_X[d]);
        let mut dir = rand & 3;
        let mut tries: i32 = 4;
        loop {
            loop {
                dir = if rand & 0x80 != 0 { dir.wrapping_add(1) } else { dir.wrapping_sub(1) } & 3;
                if dir ^ o.dir != 1 {
                    break;
                }
            }
            tries -= 1;
            if tries < 0 {
                self.slots[i].count = 0;
                return false;
            }
            if tries == 0 {
                dir = o.dir ^ 1;
            }
            let d = dir as usize;
            if !PATHS[d].contains(&look(d)) {
                continue;
            }
            let spot = look(d + 4);
            if NO_STOP.contains(&spot) {
                continue;
            }
            if (WALK_ACROSS.contains(&spot) || grid.enterable(spot)) && o.count == 0x20 {
                self.slots[i].count = 0x40;
            }
            self.slots[i].dir = dir;
            return true;
        }
    }

    /// 0x08031DDC with Mario idle at (y, x) on `tile`: the walker standing on his cell, if
    /// any (the highest slot first).
    pub fn meeting(&self, y: u8, x: u16, tile: u8) -> Option<usize> {
        if !MEETING_TILES.contains(&tile) {
            return None;
        }
        (0..SLOTS).rev().find(|&i| {
            let o = &self.slots[i];
            walker(o.kind) && o.y == y && o.x == x
        })
    }

    /// Debug: the first object of `kind` moved to (y, x), as if a new game had it there.
    pub fn place(&mut self, kind: u8, y: u8, x: u16) {
        for slots in [&mut self.slots, &mut self.spawns] {
            if let Some(o) = slots.iter_mut().find(|o| o.kind == kind) {
                (o.y, o.x, o.live_y, o.live_x) = (y, x, y, x);
            }
        }
    }

    /// Beaten in battle (0x0800F8C8): the slot is emptied.
    pub fn remove(&mut self, slot: usize) {
        self.slots[slot] = MapObject::default();
    }

    /// The walkers as drawn (0x08031DDC): frame 1 / 2 by the frame counter's bit 3 (bit 2
    /// while moving), flipped facing right (the art faces left).
    /// `hidden`: a slot not drawn (one puffing away).
    pub fn sprites(&self, frame: u32, moving: bool, hidden: Option<usize>) -> Vec<MapObjectSprite> {
        self.slots
            .iter()
            .enumerate()
            .filter(|&(i, o)| drawn(o.kind) && hidden != Some(i))
            .map(|(_, o)| o)
            .map(|o| {
                let mask = if moving && o.count != 0 { 4 } else { 8 };
                MapObjectSprite {
                    kind: o.kind,
                    y: o.live_y as i32,
                    x: o.live_x as i32,
                    frame: 1 + (frame & mask != 0) as u32,
                    flip: o.dir == 0,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world_map::tests::W1;

    /// World 1's grid (rows 0-11 = map y 0x10-0xC0).
    struct W1Grid(Vec<u8>);

    impl W1Grid {
        fn new() -> Self {
            Self(W1.iter().flat_map(|r| r.split_whitespace().map(|t| u8::from_str_radix(t, 16).unwrap())).collect())
        }
    }

    impl Grid for W1Grid {
        fn tile_at(&self, y: i32, x: i32) -> u8 {
            let (c, r) = (x >> 4, (y - 0x10) >> 4);
            if !(0..16).contains(&c) || !(0..12).contains(&r) { 0x02 } else { self.0[(r * 16 + c) as usize] }
        }
        fn enterable(&self, tile: u8) -> bool {
            tile >= [0x03, 0x67, 0xBF, 0xE9][(tile >> 6) as usize]
        }
    }

    fn world_1() -> MapObjects {
        MapObjects::new(&[
            sma4_assets::MapObjectSpawn { slot: 0, kind: 1, y: 0x70, x: 0xD0, item: 0 },
            sma4_assets::MapObjectSpawn { slot: 2, kind: HAMMER_BRO, y: 0x80, x: 0xA0, item: 9 },
        ])
    }

    #[test]
    fn the_hammer_bro_sways_on_its_cell() {
        // Recorded in mGBA on World 1's map: timer 163 -> x 0x9B, 181 -> 0xA3, 211 -> 0xA5;
        // facing 0, 1, 1.
        let mut m = world_1();
        for (t, x, dir) in [(163u8, 0x9B, 0), (181, 0xA3, 1), (211, 0xA5, 1)] {
            m.slots[0].count = t;
            m.step_idle(1);
            assert_eq!((m.slots[2].live_x, m.slots[2].dir), (x, dir), "timer {t}");
        }
    }

    /// Recorded in mGBA after a lost life in 1-1 (map sub-state 0x0C, frames 33-97): the
    /// slot's random byte each frame, from the frame the move starts.
    const RAND_SLOT_2: [u8; 64] = [
        0x9d, 0xce, 0x67, 0xb3, 0xd9, 0x6c, 0xb6, 0xdb, 0x6d, 0x36, 0x1b, 0x0d, 0x86, 0xc3, 0xe1, 0xf0,
        0xf8, 0x7c, 0xbe, 0x5f, 0xaf, 0x57, 0x2b, 0x95, 0x4a, 0xa5, 0x52, 0x29, 0x94, 0xca, 0x65, 0xb2,
        0xd9, 0xec, 0x76, 0x3b, 0x9d, 0x4e, 0x27, 0x93, 0x49, 0x24, 0x12, 0x09, 0x84, 0x42, 0x21, 0x90,
        0xc8, 0x64, 0x32, 0x19, 0x0c, 0x06, 0x03, 0x81, 0x40, 0x20, 0x10, 0x88, 0x44, 0x22, 0x11, 0x88,
    ];

    #[test]
    fn after_a_lost_life_the_hammer_bro_walks_right_and_back() {
        // Before the move: on (0x80, 0xA0), direction 3 (0x0800D5B4 set it).
        let mut m = world_1();
        m.slots[2].dir = 3;
        m.start_move();
        let grid = W1Grid::new();
        let mut xs = Vec::new();
        for (f, &r) in RAND_SLOT_2.iter().enumerate() {
            let mut rand = [0; 18];
            rand[2] = r;
            let done = m.step_move(&rand, &grid);
            xs.push(m.slots[2].live_x);
            if done {
                assert_eq!(f, 63, "a 0x40-frame move");
                break;
            }
        }
        // As recorded: right for 0x20 frames (to 0xC0), then nowhere on but back, to 0xA0.
        let recorded: Vec<u16> = (0xA1..=0xC0).chain((0xA0..0xC0).rev()).collect();
        assert_eq!(xs, recorded);
        assert_eq!((m.slots[2].y, m.slots[2].x), (0x80, 0xA0));
        assert_eq!(m.slots[2].dir, 1);
    }

    #[test]
    fn mario_meets_a_walker_on_its_junction() {
        let m = world_1();
        assert_eq!(m.meeting(0x80, 0xA0, 0x47), Some(2));
        assert_eq!(m.meeting(0x80, 0xA0, 0x45), None, "not a junction tile");
        assert_eq!(m.meeting(0x70, 0xD0, 0x47), None, "kind 1 does not walk");
    }
}
