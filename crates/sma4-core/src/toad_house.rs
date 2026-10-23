//! Toad Houses (tileset 7) and the way into World 1-3's hidden one (see docs/re-notes.md,
//! "Toad House"):
//!
//! * Trigger 0x07 (init 0x0804E460, main 0x0804E718), invisible, just before 1-3's goal:
//!   while the player is behind the scenery (0x03003D06) and its frame is within 16 px
//!   across and 32 px up or down of the trigger ((object - player) >> 10, 4 px steps), the
//!   game warps (0x03003CA0 = 5) to level 0x106 with the enemy list 0x0816BA52 and chest
//!   choice 0x03003CAF = 1.
//! * Arrival (kind 5, 0x0800578C, then the loader 0x08014F20's special case for this house's
//!   object stream 0x0815BA83): frame (0x20, start y 0x140 - 0x10), camera x 0, tileset 7.
//! * Toad (type 0x34, init 0x0806E2B8, main 0x0806E4FC): freezes the player for 0x76 frames
//!   (+0x40) and draws the message box on BG3 (phase 0 0x0806E384: one row per call), then
//!   prints the message (phase 1 0x0806E490: one byte per call every 4 frames, 0x080AB140).
//!   The main runs the phase handler twice a frame.
//! * Chest (0x0809E39C -> 0x080A7B9C): B pressed on the ground over a chest tile (collision
//!   ids 0x81-0x84) opens it, unless the frame's x low byte is in 0x58..0x60 or
//!   0x80..0x88 (between chests); the item object 0x35 (0x0804A9A0, main 0x0804FFFC)
//!   clears the message box, opens the chest (tile change 0x14), rises, blinks and ends the
//!   visit.

use sma4_assets::EnemySpawn;

use crate::input::Buttons;
use crate::level::{Level, TILE_PX};
use crate::player::Player;

pub const WARP_TRIGGER: u16 = 0x07;
pub const TOAD: u16 = 0x34;
/// World 1-3's hidden Toad House.
pub const HIDDEN_HOUSE: u16 = 0x106;
pub const TILESET: u8 = 7;
/// The player's frame on arrival (0x0800578C, 0x08014F20).
pub const ARRIVAL: (i32, i32) = (0x20, 0x130);
/// Toad's init freezes the player (+0x40 = 0x76).
pub const ARRIVAL_FREEZE: u8 = 0x76;
/// The item freezes the player while it lives (+0x40 = 2 every frame).
pub const ITEM_FREEZE: u8 = 2;

/// Warp Whistle: chest choice 0x03003CAF = 1 -> 0x08173785[0] (inventory item id).
pub const WARP_WHISTLE: u8 = 0x0C;
/// Chest x by position (0x08173772): left, middle, right.
const CHEST_X: [i32; 3] = [0x40, 0x68, 0x90];
/// The chest's top row (0x03003CBD:CBC = 0x160). When the item comes out, tile change 0x14
/// (0x08027E3C) draws the open lid there (2x2 BG tiles at the item's x, in 8 px steps) and
/// makes the cell 0x0813AA00[0x13] = 0x007, which no longer collides as a chest.
const CHEST_Y: i32 = 0x160;
/// Collision ids of the chest's cells (0x080A7B9C: 0x84 - id < 4).
const CHEST_IDS: std::ops::RangeInclusive<u8> = 0x81..=0x84;
/// Item spawn (0x0804A9A0): screen y 0x60, speed 0xD0 up, 5 frames before it moves.
const ITEM_SCREEN_Y: i32 = 0x60;
const ITEM_VY: i32 = -0x30;
const ITEM_WAIT: u8 = 5;
/// It blinks this long once it stops (+0x24 = 0x70).
const ITEM_SHOW: u8 = 0x70;
/// Frames from the end of the item to the map.
// TODO(re): the exit runs 0x08132534[0x03003CAA] (0x08005ECC, 0x0800731C: 0x0300374C - 4
// a frame to -0x90, 0x08005F44, ...); only its length is approximated here.
const EXIT_FRAMES: u16 = 0x40;

/// Message box rows (0x0806E384).
pub const BOX_ROWS: u8 = 8;
/// Text timing (0x0806E384 / 0x0806E490): the first byte 0x10 frames after the box, then one
/// every 4.
const TEXT_DELAY: u8 = 0x10;
const TEXT_STEP: u8 = 4;
const NEWLINE: u8 = 0xF0;
const SKIP: u8 = 0xFE;
const END: u8 = 0xFF;

/// The invisible warp trigger (type 0x07).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WarpTrigger {
    x: i32,
    y: i32,
}

impl WarpTrigger {
    pub fn find(list: &[EnemySpawn]) -> Option<Self> {
        list.iter()
            .find(|e| e.kind == WARP_TRIGGER)
            .map(|e| Self { x: e.col as i32 * TILE_PX, y: e.row as i32 * TILE_PX })
    }

    /// 0x0804E718: behind the scenery and close enough.
    pub fn hit(&self, p: &Player) -> bool {
        let (fx, fy) = p.frame();
        let dx = (self.x - fx).div_euclid(4);
        let dy = (self.y - fy).div_euclid(4);
        p.behind != 0 && (-4..4).contains(&dx) && (-8..8).contains(&dy)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Drawing the box: rows done.
    Box(u8),
    Text,
    Idle,
}

/// The chest's item (object 0x35).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChestItem {
    /// Inventory item id (+0x17).
    pub kind: u8,
    /// Left edge and top, px; y in 1/256 px.
    pub x: i32,
    y: i32,
    vy: i32,
    /// +0x24.
    timer: u8,
    /// Drawn this frame.
    pub visible: bool,
}

impl ChestItem {
    pub fn px(&self) -> (i32, i32) {
        (self.x, self.y >> 8)
    }
}

/// What the house did this frame.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Event {
    /// The item went into the inventory.
    pub got: Option<u8>,
    /// The visit is over: back to the map.
    pub leave: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToadHouse {
    /// Toad's frame (16x32) top-left, px: the list's cell with y = 0x100 + its low byte.
    pub toad: (i32, i32),
    /// Toad faces right (+0x1A = 0x40) while the player is at or right of him.
    pub toad_right: bool,
    phase: Phase,
    /// +0x23.
    timer: u8,
    message: Vec<u8>,
    /// Message bytes read so far.
    printed: usize,
    /// Box rows from this one down are cleared again (0x0804FF38).
    cleared_from: u8,
    pub item: Option<ChestItem>,
    opened: bool,
    /// The open lid's top-left, px, once drawn.
    pub chest_lid: Option<(i32, i32)>,
    exit: Option<u16>,
}

impl ToadHouse {
    /// The house's list (Toad) and Toad's message (BG3 tile ids, see `ToadHouse::text_cells`).
    pub fn new(list: &[EnemySpawn], message: Vec<u8>) -> Option<Self> {
        let toad = list.iter().find(|e| e.kind == TOAD)?;
        // Init: y high half = 1; the kind (+0x3A = row >> 4) picks the message (0x0816F3C4).
        let y = 0x100 | (toad.row as i32 * TILE_PX) & 0xFF;
        Some(Self {
            toad: (toad.col as i32 * TILE_PX, y),
            toad_right: false,
            phase: Phase::Box(0),
            timer: 0,
            message,
            printed: 0,
            cleared_from: BOX_ROWS,
            item: None,
            opened: false,
            chest_lid: None,
            exit: None,
        })
    }

    /// The item is out or the visit is ending: the player is held.
    pub fn busy(&self) -> bool {
        self.item.is_some() || self.exit.is_some()
    }

    /// Box rows drawn and not cleared.
    pub fn box_rows(&self) -> u8 {
        let drawn = match self.phase {
            Phase::Box(n) => n,
            _ => BOX_ROWS,
        };
        drawn.min(self.cleared_from)
    }

    /// Cells of the message printed so far, in box tiles (`sma4_assets::TOAD_TEXT_AT` first):
    /// 0xF0 starts the next row, 0xFE leaves a cell blank.
    pub fn text_cells(&self) -> Vec<(u32, u32)> {
        let (mut col, mut row) = (0, 0);
        let mut out = Vec::new();
        for &b in &self.message[..self.printed] {
            match b {
                NEWLINE => (col, row) = (0, row + 1),
                SKIP => col += 1,
                END => break,
                _ => {
                    if row + 1 < self.cleared_from as u32 {
                        out.push((col, row));
                    }
                    col += 1;
                }
            }
        }
        out
    }

    /// One frame. `pressed` / `held` are the player's buttons (cleared while frozen).
    pub fn step(
        &mut self,
        player: &mut Player,
        level: &Level,
        pressed: Buttons,
        frozen: bool,
        camera_y: i32,
        frame: u32,
    ) -> Event {
        let mut ev = Event::default();
        if let Some(t) = &mut self.exit {
            *t += 1;
            ev.leave = *t >= EXIT_FRAMES;
            return ev;
        }
        self.toad_right = self.toad.0 - player.frame().0 < 1;
        self.timer = self.timer.saturating_sub(1);
        for _ in 0..2 {
            self.step_toad();
        }
        if self.item.is_none() && !self.opened && !frozen && pressed.contains(Buttons::B) {
            self.open_chest(player, level, camera_y);
        }
        if let Some(item) = &mut self.item {
            item.timer = item.timer.saturating_sub(1);
            item.visible = false;
            if item.vy == ITEM_VY {
                if item.timer != 0 {
                    // 0x0804FF38: two rows of the box cleared a frame, bottom up; Toad
                    // stops talking.
                    self.cleared_from = self.cleared_from.min(2 * (item.timer - 1));
                    if item.timer == 4 {
                        self.phase = Phase::Idle;
                    }
                    return ev;
                }
                self.chest_lid = Some((item.x & !7, CHEST_Y));
            }
            if item.timer == 0 {
                if item.vy >= 0 {
                    // Sound 0x11; the exit starts (0x03003772, 0x03003CAA = 1).
                    self.item = None;
                    self.exit = Some(0);
                    return ev;
                }
                // 0x08049E80: up only.
                item.y += item.vy * 16;
                item.vy += 1;
                if item.vy < 0 {
                    item.visible = true;
                    return ev;
                }
                item.timer = ITEM_SHOW;
                ev.got = Some(item.kind);
            }
            item.visible = frame & 0xC != 0;
        }
        ev
    }

    /// Toad's phase handler (0x0816F440[+0x3B]).
    fn step_toad(&mut self) {
        match self.phase {
            Phase::Box(n) => {
                if n + 1 >= BOX_ROWS {
                    self.phase = Phase::Text;
                    self.timer = TEXT_DELAY;
                } else {
                    self.phase = Phase::Box(n + 1);
                }
            }
            Phase::Text => {
                if self.timer != 0 {
                    return;
                }
                let b = self.message.get(self.printed).copied().unwrap_or(END);
                self.printed += 1;
                if b == END {
                    self.phase = Phase::Idle;
                }
                self.timer = TEXT_STEP;
            }
            Phase::Idle => {}
        }
    }

    /// 0x0809E39C / 0x080A7B9C.
    fn open_chest(&mut self, player: &mut Player, level: &Level, camera_y: i32) {
        let fx = player.frame().0;
        let lo = fx & 0xFF;
        let py = player.px().1;
        // TODO(re): the probe is 0x03007820 + 0x0D; the cell at the body's lower half is
        // assumed here.
        let probe = level.collision_id((fx + 8).div_euclid(TILE_PX), (py + player.height() - 8).div_euclid(TILE_PX));
        if !player.on_ground
            || !probe.is_some_and(|id| CHEST_IDS.contains(&id))
            || (0x58..0x60).contains(&lo)
            || (0x80..0x88).contains(&lo)
        {
            return;
        }
        let i = if lo < 0x60 {
            0
        } else if lo < 0x88 {
            1
        } else {
            2
        };
        player.vx = 0;
        self.opened = true;
        self.item = Some(ChestItem {
            kind: WARP_WHISTLE,
            x: CHEST_X[i],
            y: (camera_y + ITEM_SCREEN_Y) << 8,
            vy: ITEM_VY,
            timer: ITEM_WAIT,
            visible: false,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn house() -> ToadHouse {
        let list = [EnemySpawn { kind: TOAD, col: 0x0C, row: 0x03, ..Default::default() }];
        // "Hi" / "!"
        ToadHouse::new(&list, vec![0x10, 0x11, NEWLINE, SKIP, 0x12, END]).unwrap()
    }

    #[test]
    fn toad_stands_at_y_0x100_plus_the_rows_low_byte() {
        assert_eq!(house().toad, (0xC0, 0x130));
    }

    #[test]
    fn box_takes_four_frames_then_text_one_byte_every_four() {
        let mut h = house();
        h.step_toad();
        h.step_toad();
        assert_eq!(h.box_rows(), 2);
        for _ in 0..3 {
            h.step_toad();
            h.step_toad();
        }
        assert_eq!(h.box_rows(), BOX_ROWS);
        assert_eq!(h.phase, Phase::Text);
        let mut frames = 0;
        while h.phase != Phase::Idle {
            h.timer = h.timer.saturating_sub(1);
            h.step_toad();
            h.step_toad();
            frames += 1;
        }
        assert_eq!(frames, TEXT_DELAY as u32 + 5 * TEXT_STEP as u32);
        assert_eq!(h.text_cells(), [(0, 0), (1, 0), (1, 1)]);
    }
}
