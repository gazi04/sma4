//! The King's throne room (World 1's castle, level 0x1C5): list type 0x1A9 counts up
//! 0x03003D12 (0x0804B768), and while it is set the object loop runs 0x08093CBC every
//! frame: the first time the init 0x08093CE4, then 0x08093E18. Toad hops in panic, the
//! changed King paces in front of his throne, a message box opens over BG3 and Toad's
//! message prints letter by letter; after it, A or B (400 frames after the text started)
//! sends Mario on to the airship (0x08094140: the area switch 0x03003CA0 = 3 to the
//! room's junction, level 0x202). Checked against mGBA frame by frame (OAM, the slots'
//! bytes, the counters). See docs/re-notes.md "King's castle".

use sma4_assets::CastleAsset;

use crate::input::Buttons;
use crate::obj::ObjSprite;


/// List type of the throne room.
pub const THRONE: u16 = 0x1A9;

/// Before the level loop runs (sub-states 0x17-0x19, the fade-in) the objects are held
/// (+0xBD1) while the throne's counters and draw already run: the port's level starts at
/// sub-state 0x1A, this many frames after the init.
pub const FADE_IN: u32 = 30;
/// The delay before the box (0x0300384A = 0x20) and before the first letter (0x10).
const BOX_DELAY: u8 = 0x20;
const TEXT_DELAY: u8 = 0x10;
/// A letter every 4 frames; a skipped cell takes 1.
const LETTER: u8 = 4;
const SKIP_DELAY: u8 = 1;
pub const BOX_ROWS: u8 = 8;
/// After the text starts, A / B count only once 0x0300760C passes this.
const WAIT: u16 = 400;
const NEWLINE: u8 = 0xF0;
const SKIP: u8 = 0xFE;
const END: u8 = 0xFF;

/// Tables (`sma4_assets::CASTLE_ROM`).
const TOAD_TILES: u32 = 0x0817_1EE4;
const KING_OFFSETS: u32 = 0x0817_1EE7;
const KING_X: u32 = 0x0817_1EEF;
const KING_Y: u32 = 0x0817_1EF6;
const ANIM_BASE: u32 = 0x0817_1EFD;
const ANIM_TIMES: u32 = 0x0817_1F04;
const ANIM_SETS: u32 = 0x0817_1F3C;
const KING_TILES: u32 = 0x0817_1F74;
const KING_ATTRS: u32 = 0x0817_2024;
const EXTRA_DY: u32 = 0x0817_207C;
const EXTRA_DX: u32 = 0x0817_2092;
/// Toad's OAM entry (0x03002FC8), the King's four (0x03003030), the extra piece
/// (0x03003050 in World 1).
const TOAD_OAM: usize = 64;
const KING_OAM: usize = 77;
const EXTRA_OAM: usize = 81;

/// Map object kind of the airship waiting to be boarded (map slot 1 after the throne room,
/// 0x08094140).
pub const MAP_AIRSHIP: u8 = 2;

/// Where the throne room sends Mario.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Leave {
    /// The airship is still at the castle (map slot 0 of the world, kind 1): the area
    /// switch to the airship's intro; the map's slot 1 becomes kind 2.
    Airship,
    /// It got away: back to the map (0x03003851 = 0, sub-state 0x1B).
    Map,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Throne {
    world: u8,
    /// The map's airship is at the castle (0x03002BBE[world * 14] != 0).
    airship: bool,
    /// 0x0300384A.
    delay: u8,
    /// Box rows written (0x03003BAC).
    rows: u8,
    /// Phase (slot 0 +0x3B): 0 the box, 1 the text, 2 waiting for A / B.
    phase: u8,
    /// 0x0300760C.
    count: u16,
    message: Vec<u8>,
    printed: usize,
    /// Slot 0, Toad: height (+0, 1/256 px, up negative), speed +0x10, frame +0x17, timer
    /// +0x23, the King's animation +0x35, +0x3F.
    toad_h: i32,
    toad_v: i8,
    toad_frame: u8,
    toad_timer: u8,
    anim: u8,
    hold: u8,
    /// Slot 1, the King: x (+0x70, 1/256 px from his place), speed +0x80, facing +0x87.
    king_x: i32,
    king_v: i8,
    king_right: u8,
    /// 0x03003BB2 (animation timer) and 0x03003BB3.
    anim_timer: u8,
    lift: u8,
    /// The OAM shadow entries this room writes (attr0, attr1, attr2).
    oam: Vec<[u16; 3]>,
}

impl Throne {
    /// 0x08093CE4 (the init), then the fade-in's held frames.
    pub fn new(data: &CastleAsset, world: u8, airship: bool) -> Self {
        let message = data
            .message(if airship { sma4_assets::THRONE_MESSAGE } else { sma4_assets::HURRY_MESSAGE })
            .to_vec();
        let mut t = Self {
            world,
            airship,
            delay: BOX_DELAY,
            rows: 0,
            phase: 0,
            count: 0,
            message,
            printed: 0,
            toad_h: 0,
            toad_v: 0,
            toad_frame: 0,
            toad_timer: 0,
            anim: data.rom_u8(ANIM_BASE + world as u32),
            hold: if world == 1 { 0xFF } else { 0 },
            king_x: 0,
            king_v: -0x10,
            king_right: 0,
            anim_timer: 0,
            lift: 0,
            oam: vec![[0; 3]; 128],
        };
        for _ in 0..FADE_IN {
            t.frame(data, Buttons::empty(), true);
        }
        t
    }

    /// One frame of 0x08093E18; `pressed` A / B.
    pub fn step(&mut self, data: &CastleAsset, pressed: Buttons) -> Option<Leave> {
        self.frame(data, pressed, false)
    }

    fn frame(&mut self, data: &CastleAsset, pressed: Buttons, held: bool) -> Option<Leave> {
        self.delay = self.delay.saturating_sub(1);
        let mut leave = None;
        if !held {
            // The object loop's generic timer (+0x23) runs before the throne's handler.
            self.toad_timer = self.toad_timer.saturating_sub(1);
            self.toad();
            self.king();
            leave = self.phase(pressed);
        }
        self.draw(data);
        leave
    }

    /// 0x080946EC: Toad hops (speed -0x28, +4 a frame), 7 frames down, 7 frames up.
    fn toad(&mut self) {
        if self.toad_frame != 1 {
            if self.toad_timer != 0 {
                return;
            }
            if self.toad_frame != 0 {
                self.toad_frame = 0;
                self.toad_timer = 7;
                return;
            }
            self.toad_frame = 1;
            self.toad_v = -0x28;
            self.toad_h &= !0xFF00;
        }
        self.toad_v = self.toad_v.wrapping_add(4);
        self.toad_h += self.toad_v as i32 * 16;
        if (self.toad_h >> 8) & 0xFF == 0 {
            self.toad_v = 0;
            self.toad_h &= !0xFF;
            self.toad_frame = 2;
            self.toad_timer = 7;
        }
    }

    /// The King's handler by world (0x081720B0): World 1's 0x0809461C paces 32 px left
    /// and back at 1 px a frame.
    fn king(&mut self) {
        // TODO(re): the other worlds' Kings (0x08094664 and 0x080946E8) are not ported.
        self.king_x += self.king_v as i32 * 16;
        match (self.king_x >> 8) as u8 {
            0xE0 => {
                self.king_v = 0x10;
                self.king_x &= !0xFF;
                self.king_right = 1;
            }
            0 => {
                self.king_v = -0x10;
                self.king_x &= !0xFF;
                self.king_right = 0;
            }
            _ => {}
        }
    }

    /// The phase handler 0x08171E04[+0x3B].
    fn phase(&mut self, pressed: Buttons) -> Option<Leave> {
        let ab = pressed.contains(Buttons::A) || pressed.contains(Buttons::B);
        match self.phase {
            // 0x08093EA0: one box row a frame once the delay is over.
            0 => {
                if self.delay == 0 && self.hold < 0x80 {
                    self.rows += 1;
                    if self.rows >= BOX_ROWS {
                        self.delay = TEXT_DELAY;
                        self.phase = 1;
                        self.count = 0;
                    }
                }
                None
            }
            // 0x08094040: the next byte of the message (0x080AB140).
            1 => {
                self.count += 1;
                if self.delay != 0 {
                    return None;
                }
                let b = self.message.get(self.printed).copied().unwrap_or(END);
                self.printed += 1;
                if b == END {
                    self.phase = 2;
                }
                if !self.airship && ab {
                    return Some(Leave::Map);
                }
                self.delay = if b == SKIP { SKIP_DELAY } else { LETTER };
                None
            }
            // 0x08094104.
            _ => {
                self.count += 1;
                (self.count > WAIT && ab).then_some(if self.airship { Leave::Airship } else { Leave::Map })
            }
        }
    }

    /// 0x080941B8: Toad, the King's four pieces and the extra one into the OAM shadow.
    fn draw(&mut self, data: &CastleAsset) {
        let w = self.world as u32;
        let toad = &mut self.oam[TOAD_OAM];
        toad[0] = (toad[0] & 0xFF00) | ((self.toad_h >> 8) as u8).wrapping_add(0x59) as u16;
        toad[2] = 0x5400 | data.rom_u8(TOAD_TILES + self.toad_frame as u32) as u16;
        toad[1] = (toad[1] & 0x1E00) | 0x9098;
        let base = data.rom_u8(ANIM_BASE + w);
        let mut idx = self.anim;
        if self.anim_timer == 0 {
            idx = idx.wrapping_sub(1);
            if (idx.wrapping_sub(base) as i8) < 0 {
                idx = base.wrapping_add(7);
            }
            self.anim = idx;
            // TODO(re): World 4 takes (random & 7) + 6.
            self.anim_timer = data.rom_u8(ANIM_TIMES + idx as u32);
        }
        let facing = if self.world == 1 || self.king_right == 0 { 0 } else { 0x10 };
        let set = data.rom_u8(ANIM_SETS + idx as u32).wrapping_add(facing);
        let bx = ((self.king_x >> 8) as u8).wrapping_add(data.rom_u8(KING_X + w));
        let by = self.lift.wrapping_add(data.rom_u8(KING_Y + w)).wrapping_sub(0x18);
        for k in 0..4u32 {
            let e = &mut self.oam[KING_OAM + k as usize];
            let (dx, dy) = (data.rom_u8(KING_OFFSETS + k * 2), data.rom_u8(KING_OFFSETS + k * 2 + 1));
            e[1] = (e[1] & 0xFE00) | (bx.wrapping_add(dx).wrapping_sub(0x10) as u16);
            e[0] = (e[0] & 0xFF00) | by.wrapping_add(dy) as u16;
            let tile = data.rom_u16(KING_TILES + (set as u32 + k) * 2) & 0x3FF;
            let a = data.rom_u8(KING_ATTRS + set as u32 + k) as u16;
            e[2] = (e[2] & 0xFC00) | tile;
            e[2] = (e[2] & 0x03FF) | (3 - ((a >> 4) & 3)) << 10 | ((a >> 1) & 7) << 12;
            e[1] = (e[1] & 0x0FFF) | (a >> 7) << 13 | ((a >> 6) & 1) << 12 | 0x4000;
            e[0] &= 0x3FFF;
        }
        let (x0, y0) = (self.oam[KING_OAM][1] & 0xFF, self.oam[KING_OAM][0] & 0xFF);
        self.anim_timer = self.anim_timer.wrapping_sub(1);
        let u = set >> 2;
        let flip = if u.wrapping_sub(2) < 2 || u > 0x11 { 0x40u16 } else { 0 };
        // TODO(re): Worlds 2, 4, 5, 7's extra piece goes to entry 76 and World 2 hangs a
        // rope (entries 91-95); only World 1's layout is ported.
        let e = &mut self.oam[EXTRA_OAM];
        e[1] = (e[1] & 0xFE00) | ((data.rom_u8(EXTRA_DX + u as u32) as i8 as i16 as u16).wrapping_add(x0) & 0x1FF);
        e[0] = (e[0] & 0xFF00) | (data.rom_u8(EXTRA_DY + u as u32).wrapping_add(y0 as u8)) as u16;
        e[2] = (e[2] & 0xFC00) | if u == 7 { 0x18D } else { 0x18E };
        let a = flip | 0x23;
        e[2] = (e[2] & 0x03FF) | (3 - ((a >> 4) & 3)) << 10 | ((a >> 1) & 7) << 12;
        e[1] = (e[1] & 0x0FFF) | (flip >> 7) << 13 | ((flip & 0x40) >> 6) << 12 | 0x4000;
        e[0] &= 0x3FFF;
    }

    /// The sprites in OAM order (front first).
    pub fn sprites(&self) -> Vec<ObjSprite> {
        [TOAD_OAM, KING_OAM, KING_OAM + 1, KING_OAM + 2, KING_OAM + 3, EXTRA_OAM]
            .iter()
            .filter_map(|&i| ObjSprite::from_attrs(self.oam[i][0], self.oam[i][1], self.oam[i][2]))
            .collect()
    }

    /// The OAM entries this room wrote (attr0, attr1, attr2), by index.
    pub fn oam(&self, index: usize) -> [u16; 3] {
        self.oam[index]
    }

    /// Box rows drawn (`sma4_assets::CASTLE_BOX_H` when done).
    pub fn box_rows(&self) -> u8 {
        self.rows.min(BOX_ROWS)
    }

    /// The box is shown (BG3 on from the init).
    pub fn text_cells(&self) -> Vec<(u32, u32)> {
        let (mut col, mut row) = (0, 0);
        let mut out = Vec::new();
        for &b in &self.message[..self.printed.min(self.message.len())] {
            match b {
                NEWLINE => (col, row) = (0, row + 1),
                SKIP => col += 1,
                END => break,
                _ => {
                    out.push((col, row));
                    col += 1;
                }
            }
        }
        out
    }

    /// The message shown (`sma4_assets::castle_text_path`).
    pub fn message_id(&self) -> u8 {
        if self.airship { sma4_assets::THRONE_MESSAGE } else { sma4_assets::HURRY_MESSAGE }
    }
}

/// The screen between the castle's rooms (mGBA, frames after the switch): the fade out
/// (sub-state 0x1D, 0x0800373C: 4 off each colour channel every other frame), the area
/// switch 0x08005C4C (0x1E) loading the next room on its first frame, then for the
/// airship's intro (0x0800515C -> 0x080244F4) "MARIO START!" on black for the rest of
/// 0x1E, black (0x17, 0x18) and an iris opening on the room (0x19, 0x08028E80, as the spade
/// panel's way in); into the airship itself, black until the room starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Curtain {
    pub kind: CurtainKind,
    /// Frames since the switch.
    pub frame: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurtainKind {
    /// Throne room -> the airship's intro.
    MarioStart,
    /// The intro -> the airship.
    Plain,
}

const CURTAIN_FADE: u16 = 15;
/// "MARIO START!": its three 32x16 OBJs (0x08024384), tiles 0x390 / 0x394 / 0x398.
const START_TEXT: [(i32, u16); 3] = [(0x48, 0x390), (0x68, 0x394), (0x88, 0x398)];
const START_TEXT_Y: i32 = 0x44;

/// What the curtain asks for this frame.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CurtainStep {
    /// Load the next room now.
    pub load: bool,
    /// Over: the room runs this frame.
    pub done: bool,
}

impl Curtain {
    pub fn new(kind: CurtainKind) -> Self {
        Self { kind, frame: 0 }
    }

    fn timing(&self) -> (u16, u16) {
        // (load frame, first frame of the room)
        match self.kind {
            CurtainKind::MarioStart => (16, 156),
            CurtainKind::Plain => (17, 70),
        }
    }

    pub fn step(&mut self) -> CurtainStep {
        self.frame += 1;
        let (load, done) = self.timing();
        CurtainStep { load: self.frame == load, done: self.frame >= done }
    }

    /// While fading out: how much has come off each colour channel (of 31); 31 while the
    /// screen is black; `None` once the iris opens.
    pub fn fade(&self) -> Option<u8> {
        if self.frame <= CURTAIN_FADE {
            return Some((self.frame.div_ceil(2) * 4).min(31) as u8);
        }
        (self.iris().is_none()).then_some(31)
    }

    /// The iris's radius once it opens (8 a frame, drawn with the same lag as the spade
    /// panel's: radius 8k - 17).
    pub fn iris(&self) -> Option<i32> {
        match self.kind {
            CurtainKind::MarioStart if self.frame >= 125 => Some(8 * (self.frame as i32 - 125) - 17),
            _ => None,
        }
    }

    /// "MARIO START!" while it shows (0x1E after the load).
    pub fn sprites(&self) -> Vec<ObjSprite> {
        if self.kind != CurtainKind::MarioStart || !(17..67).contains(&self.frame) {
            return Vec::new();
        }
        START_TEXT
            .iter()
            .map(|&(x, tile)| ObjSprite { x, y: START_TEXT_Y, tile, shape: 1, size: 2, pal: 1, prio: 1, ..Default::default() })
            .collect()
    }
}
