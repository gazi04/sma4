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
//!   effect runs (0x0813249C[item]); 8 (0x08007524): the map goes on.
//!
//! Effects: items 1-8 and 14 (0x080062BC) set the map form 0x03003CB6 = 0x0813ACF0[item]
//! (Mario's form in the next level), flash his palette and play 0x0813ACD0[item]; the
//! Starman (9, 0x08006454) gives the next level a star; the Warp Whistle (12, 0x08006850)
//! calls the whirlwind (`crate::warp`). The used item leaves the list (0x080061E8).

use crate::input::Buttons;

/// Inventory slots (0x03002C2E, player 1: 36) and their rows.
pub const SLOTS: usize = 36;
pub const PER_ROW: usize = 9;
/// Item ids.
pub const MUSHROOM: u8 = 1;
pub const FIRE_FLOWER: u8 = 2;
pub const LEAF: u8 = 3;
pub const STARMAN: u8 = 9;
pub const WARP_WHISTLE: u8 = 0x0C;
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
}

/// What closing the panel did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Used {
    /// A power-up: Mario's form id (0x0813ACF0).
    Form(u8),
    Starman,
    WarpWhistle,
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
}

/// What an item does from the panel in this port (0x081324F4 checks, 0x0813249C effects).
/// Forms the port does not have (frog, tanooki, hammer suit, cloud, P-wing) and the items
/// whose effect is not ported (anchor, hammer, music box) are refused.
pub fn effect(item: u8) -> Option<Used> {
    match item {
        MUSHROOM | FIRE_FLOWER | LEAF => Some(Used::Form(item)),
        STARMAN => Some(Used::Starman),
        WARP_WHISTLE => Some(Used::WarpWhistle),
        _ => None,
    }
}

impl Inventory {
    /// L on the map (0x08005ECC runs that frame).
    pub fn open() -> Self {
        Self { state: State::BarDown, vofs: BAR, panel_map: false, row: 0, col: 0, chosen: None }
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
                    if effect(it).is_some() {
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
                        out.used = effect(it);
                    }
                }
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
        let mut inv = Inventory { state: State::Menu, vofs: PANEL_OPEN, panel_map: true, row: 0, col: 0, chosen: None };
        run(&mut inv, &mut items, Buttons::LEFT);
        assert_eq!(inv.col, 2, "left wraps round to the last item");
        run(&mut inv, &mut items, Buttons::LEFT);
        assert_eq!(inv.col, 1);
        assert!(run(&mut inv, &mut items, Buttons::A).refused, "the anchor's effect is not ported");
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
