//! The map's item panel (L on the map; see docs/re-notes.md, "Item panel"). L pauses the
//! map (0x0800D5B4: 0x03003772 ^= 1, sound 0x1E) and the panel runs through 0x08132534
//! [0x03003CAA], moving BG0 (0x0300374C, the status bar's layer) about:
//!
//! * 0 (0x08005ECC): cursor at the first item (row 0x03003CAB = 0, column 0x03003CAD = 0,
//!   x 0x03003CAC = 0x18). 1 (0x0800731C): the bar slides down, BG0 VOFS - 4 a frame from
//!   -0x78 to -0x90, then -0x77 with the panel's map (LZ77 0x0821F674) in BG0. 2, 3
//!   (0x08005F44, 0x080069FC -> 0x0800602C): the item icons (BG entries 0x0813AC28[item]).
//!   4 (0x0800737C): the panel rises, VOFS + 8 a frame to -0xF.
//! * 5 (0x08006E48): B or L closes (sound 0x1E); Up / Down move a row (9 items) if it has
//!   items, Left / Right a column (wrapping round the 9, skipping empty slots; sound 0x45);
//!   A uses the item if its check (0x081324F4[item]) lets it, otherwise sound 0x1A. The
//!   cursor (0x08006A80) is the item itself in colour, a 16x16 OBJ at (0x03003CAC - 8,
//!   0x081324EC[row]).
//! * 6 (0x08007458): the panel goes down, VOFS - 8 a frame to -0x77, then the bar's map is
//!   back at -0x90; 7 (0x080074C4): it rises, + 4 a frame to -0x78, and the chosen item's
//!   effect runs (0x0813249C[item]); 8 (0x08007524): the map goes on once 0x03003CB1 is 0.
//!
//! Checks (0x081324F4[item], refused with sound 0x1A): the frog suit while 0x03003A74 bit 4
//! is set (0x08006C80; never in the port), the Starman while the next level already has one
//! (0x08006CA8), the anchor unless an airship (map object kind 2) is on the map and this
//! world's anchor (0x03002D4C bit) is unused, in Worlds 1-8 (0x08006CC8), the hammer unless
//! a rock (tile 0x51 / 0x52) is next to Mario (0x08006D20). The music box (13) and items 14
//! and 15 are not ported (refused).
//!
//! Effects: items 1-8 (0x080062BC) set the map form 0x03003CB6 = 0x0813ACF0[item] (7 the
//! cloud, 8 the P-Wing) and, but for the cloud, the level form 0x03002CE2 (the P-Wing's is
//! the raccoon's), flash his palette and play 0x0813ACD0[item]; the Starman (9,
//! 0x08006454) gives the next level a star; the anchor (10, 0x080064C4) sets this world's
//! bit; the hammer (11, 0x08006610) breaks the rock (`WorldMap::break_rock`); the Warp
//! Whistle (12, 0x08006850) calls the whirlwind (`crate::warp`). The forms and the hammer
//! set 0x03003CB1 = 0x14: a puff (0x08005E10) at the map spot +0x63 / +0x64 (Mario, or the
//! rock) for that many frames, the map held meanwhile. The used item leaves the list
//! (0x080061E8).

use crate::input::Buttons;

/// Inventory slots (0x03002C2E, player 1: 36) and their rows.
pub const SLOTS: usize = 36;
pub const PER_ROW: usize = 9;
/// Item ids.
pub const MUSHROOM: u8 = 1;
pub const FIRE_FLOWER: u8 = 2;
pub const LEAF: u8 = 3;
pub const FROG_SUIT: u8 = 4;
pub const TANOOKI_SUIT: u8 = 5;
pub const HAMMER_SUIT: u8 = 6;
pub const CLOUD: u8 = 7;
pub const P_WING: u8 = 8;
pub const STARMAN: u8 = 9;
pub const ANCHOR: u8 = 0x0A;
pub const HAMMER: u8 = 0x0B;
pub const WARP_WHISTLE: u8 = 0x0C;
/// Map forms (0x03003CB6) of items 0-8 (0x0813ACF0): the form number, 7 the cloud, 8 the
/// P-Wing.
pub const MAP_FORMS: [u8; 9] = [0, 1, 2, 3, 4, 5, 6, 7, 8];
pub const MAP_FORM_CLOUD: u8 = 7;
pub const MAP_FORM_P_WING: u8 = 8;
/// 0x03003CB1 after a form or the hammer: the puff's frames.
pub const PUFF_FRAMES: u8 = 0x14;
/// Puff tiles 0x0813AD24[(t & 0x1C) >> 2] by 0x03003CB1 t (0x08005E10): 0x1E, 0x1C, 0x1A,
/// 0x18, 0x16, 0x16; as cells of `sma4_assets::map_puff_path` (tiles 0x16, 0x18, 0x1A,
/// 0x1C, 0x1E).
const PUFF_CELLS: [u32; 6] = [4, 3, 2, 1, 0, 0];
/// BG0 vertical offsets (0x0300374C, signed): the bar at the bottom, slid out, the panel
/// just in, the panel open.
pub const BAR: i16 = -0x78;
const BAR_OUT: i16 = -0x90;
const PANEL_IN: i16 = -0x77;
pub const PANEL_OPEN: i16 = -0x0F;
/// Cursor (0x08006A80): x = 0x03003CAC - 8 = 24 * column + 16, y 0x081324EC[row].
pub const CURSOR_Y: [i32; 4] = [0x3F, 0x57, 0x6F, 0x87];
pub const CURSOR_X0: i32 = 0x10;
pub const CURSOR_DX: i32 = 0x18;
/// Panel icons in BG0 (0x0800602C): the item at row r, column c at BG0 line 48 + 24r,
/// x 16 + 24c (16x16).
pub const ICON_LINE0: i32 = 48;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// 0x03003CAA 1: the bar going down.
    BarDown,
    /// 2, 3: the panel and its icons put in (a frame each).
    Fill(u8),
    /// 4: the panel coming up.
    Rising,
    /// 5.
    Menu,
    /// 6: the panel going down.
    Lowering,
    /// 7: the bar coming back.
    BarUp,
    /// 8: the effect's puff (0x03003CB1, counted down by 0x08005E10) before the map goes on.
    Puff(u8),
}

/// What closing the panel did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Used {
    /// A power-up, suit, the cloud or the P-Wing: the map form (0x0813ACF0).
    Form(u8),
    Starman,
    Anchor,
    Hammer,
    WarpWhistle,
}

/// What the item checks (0x081324F4) see on the map.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Checks {
    /// The next level already starts with a star (player +0x5C).
    pub star_pending: bool,
    /// An airship waits on the map and this world's anchor is unused (0x08006CC8).
    pub anchor: bool,
    /// A rock is next to Mario (0x08006D20).
    pub rock: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inventory {
    pub state: State,
    /// BG0 VOFS.
    pub vofs: i16,
    /// The panel's map is in BG0 (else the bar's).
    pub panel_map: bool,
    /// 0x03003CAB / 0x03003CAD.
    pub row: usize,
    pub col: usize,
    /// The item to use once the panel has closed (map +0x68).
    chosen: Option<u8>,
    /// The item checks, set by the game before each step.
    pub checks: Checks,
}

/// One frame's outcome.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Step {
    /// The panel is gone: the map goes on.
    pub done: bool,
    /// The chosen item, taken out of the list, to apply.
    pub used: Option<Used>,
    /// A refused item (sound 0x1A).
    pub refused: bool,
    /// The effect's puff drawn this frame: a cell of `sma4_assets::map_puff_path`.
    pub puff: Option<u32>,
}

/// What an item does from the panel (0x081324F4 checks, 0x0813249C effects); `None` is
/// refused.
pub fn effect(item: u8, checks: &Checks) -> Option<Used> {
    match item {
        MUSHROOM..=P_WING => Some(Used::Form(MAP_FORMS[item as usize])),
        STARMAN if !checks.star_pending => Some(Used::Starman),
        ANCHOR if checks.anchor => Some(Used::Anchor),
        HAMMER if checks.rock => Some(Used::Hammer),
        WARP_WHISTLE => Some(Used::WarpWhistle),
        _ => None,
    }
}

impl Inventory {
    /// L on the map (0x08005ECC runs that frame).
    pub fn open() -> Self {
        Self { state: State::BarDown, vofs: BAR, panel_map: false, row: 0, col: 0, chosen: None, checks: Checks::default() }
    }

    /// The cursor's slot.
    pub fn slot(&self) -> usize {
        self.row * PER_ROW + self.col
    }

    /// One frame; `items` is the inventory (0 = empty), taken from when an item is used.
    pub fn step(&mut self, pressed: Buttons, items: &mut Vec<u8>) -> Step {
        let mut out = Step::default();
        let item = |i: usize| items.get(i).copied().unwrap_or(0);
        match self.state {
            State::BarDown => {
                self.vofs -= 4;
                if self.vofs == BAR_OUT {
                    self.vofs = PANEL_IN;
                    self.panel_map = true;
                    self.state = State::Fill(0);
                }
            }
            State::Fill(n) => self.state = if n == 0 { State::Fill(1) } else { State::Rising },
            State::Rising => {
                self.vofs += 8;
                if self.vofs == PANEL_OPEN {
                    self.state = State::Menu;
                }
            }
            State::Menu => {
                if pressed.contains(Buttons::B) || pressed.contains(Buttons::L) {
                    self.state = State::Lowering;
                    return out;
                }
                // Up / Down: only with a second row (slot 9 used); a row must have items.
                if item(PER_ROW) != 0 {
                    if pressed.contains(Buttons::DOWN) && self.row < 3 && item((self.row + 1) * PER_ROW) != 0 {
                        self.row += 1;
                    } else if pressed.contains(Buttons::UP) && self.row > 0 {
                        self.row -= 1;
                    }
                    while self.col > 0 && item(self.slot()) == 0 {
                        self.col -= 1;
                    }
                }
                // Left / Right: only with a second item; wrap round the row, skip empties.
                if item(1) != 0 && (pressed.contains(Buttons::LEFT) || pressed.contains(Buttons::RIGHT)) {
                    let step = if pressed.contains(Buttons::RIGHT) { 1 } else { PER_ROW - 1 };
                    loop {
                        self.col = (self.col + step) % PER_ROW;
                        if item(self.slot()) != 0 {
                            break;
                        }
                    }
                }
                if pressed.contains(Buttons::A) {
                    let it = item(self.slot());
                    if effect(it, &self.checks).is_some() {
                        self.chosen = Some(it);
                        self.state = State::Lowering;
                    } else {
                        out.refused = true;
                    }
                }
            }
            State::Lowering => {
                self.vofs -= 8;
                if self.vofs == PANEL_IN {
                    self.vofs = BAR_OUT;
                    self.panel_map = false;
                    self.state = State::BarUp;
                }
            }
            State::BarUp => {
                self.vofs += 4;
                if self.vofs == BAR {
                    out.done = true;
                    if let Some(it) = self.chosen.take() {
                        // 0x080061E8: the list closes up behind it.
                        let slot = self.slot();
                        if slot < items.len() {
                            items.remove(slot);
                        }
                        out.used = effect(it, &self.checks);
                        // The forms and the hammer: the puff, the map held (0x08007524).
                        if matches!(out.used, Some(Used::Form(_) | Used::Hammer)) {
                            out.done = false;
                            self.state = State::Puff(PUFF_FRAMES);
                        }
                    }
                }
            }
            State::Puff(t) => {
                // 0x08005E10 draws with t and counts down, then 0x08007524 sees 0.
                out.puff = Some(PUFF_CELLS[((t & 0x1C) >> 2) as usize]);
                self.state = State::Puff(t - 1);
                out.done = t == 1;
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(inv: &mut Inventory, items: &mut Vec<u8>, keys: Buttons) -> Step {
        inv.step(keys, items)
    }

    /// mGBA: L at frame 0, BG0 VOFS -0x7C at 2 ... -0x8C at 6, -0x77 at 7, + 8 from 10 to
    /// -0xF at 22; the menu from 23.
    #[test]
    fn opens_like_the_rom() {
        let mut items = vec![MUSHROOM, WARP_WHISTLE];
        let mut inv = Inventory::open();
        let mut vofs = vec![];
        while inv.state != State::Menu {
            run(&mut inv, &mut items, Buttons::empty());
            vofs.push(inv.vofs);
        }
        assert_eq!(&vofs[..6], &[-0x7C, -0x80, -0x84, -0x88, -0x8C, PANEL_IN]);
        assert_eq!(vofs.len(), 6 + 2 + 13);
        assert_eq!(*vofs.last().unwrap(), PANEL_OPEN);
    }

    #[test]
    fn the_cursor_skips_empty_slots_and_a_uses_the_item_after_closing() {
        let mut items = vec![MUSHROOM, 0x0A, WARP_WHISTLE];
        let mut inv = Inventory { state: State::Menu, panel_map: true, vofs: PANEL_OPEN, ..Inventory::open() };
        run(&mut inv, &mut items, Buttons::LEFT);
        assert_eq!(inv.col, 2, "left wraps round to the last item");
        run(&mut inv, &mut items, Buttons::LEFT);
        assert_eq!(inv.col, 1);
        assert!(run(&mut inv, &mut items, Buttons::A).refused, "no airship on the map: the anchor is refused");
        run(&mut inv, &mut items, Buttons::RIGHT);
        run(&mut inv, &mut items, Buttons::A);
        let mut last = Step::default();
        let mut n = 0;
        while !last.done {
            last = run(&mut inv, &mut items, Buttons::empty());
            n += 1;
        }
        // mGBA: 13 frames down, 6 up.
        assert_eq!(n, 13 + 6);
        assert_eq!(last.used, Some(Used::WarpWhistle));
        assert_eq!(items, [MUSHROOM, 0x0A]);
    }
}
