//! After a world's castle (World 1's): the level's end after Mario falls through the sky,
//! the throne room again with the King changed back, the Princess's letter (her item into
//! the inventory) and the "WORLD n CLEAR!" save screen, then the next world's map. See
//! docs/re-notes.md "After the castle".
//!
//! * Mode 1 sub-states (0x081318CC): 0x1B the palette fades out (0x0800373C), 0x1C
//!   (0x080277EC) with 0x0300386B set: 0x0300378F + 1 and 0x08017170 (the map node's level
//!   again, 0x03003B0A = 0x1C5), 0x1F (0x080297E4: the font into BG1's chars, then the
//!   scene's init 0x08029918), 0x20 (0x080298C4: BLDY 16 down to 0, a step every other
//!   frame, the sprites drawn), 0x21 (0x0802A98C: the scene, 0x0813F910[0x0300385C]).
//! * 0x0300385C: 1 (0x08029A40) the box, a row a frame from 0x0813F680 once 0x03003850 is
//!   0 (Mario's fall holds it), then message 0x0813F6A8[form]; 2 (0x08029B44) the text, a
//!   letter every 4 frames; 3 (0x08029DB0) 0xFF frames, then BLDY up a step every 4 frames
//!   and the letter (0x08007BD0, 0x0802F214) with the world's item 0x0813F8A8[world]
//!   (0x08029C14); 4 (0x0802A0A8) the letter until A or B: sub-state 0x22 (0x0802AA34:
//!   the palette fades out, the world + 1), 0x48 / 0x49 the save screen (0x08039C30,
//!   0x0803AE48), then 0x0A: the next world's map.
//! * Each 0x21 frame: 0x03003850 - 1, 0xBBD - 1, the OAM buffer cleared, the step, then the
//!   King, Toad and the wand (0x0802A404) and Mario (0x0802A8C0).

use std::sync::Arc;

use sma4_assets::EndingAsset;

use crate::input::Buttons;
use crate::obj::ObjSprite;
use crate::title::Video;

/// The map node's level the castle's end goes back to (World 1: the throne room).
pub const THRONE_LEVEL: u16 = 0x1C5;
pub const THRONE_TILESET: u8 = 2;
pub const THRONE_ENEMIES: u16 = 0xCE;

/// Frames after the level's end (mGBA): the fade out (0x1B, 15 frames from frame 1), black
/// while the room loads (0x1C..0x1F), the scene's init at `LOAD`.
const FADE_OUT: u16 = 15;
pub const LOAD: u16 = 60;

/// The scene's cameras: the BGs at 0x108 (0x03003754), the objects at 0x20 (0x03003CE0).
pub const BG_CAMERA_Y: i32 = 0x108;
pub const OBJ_CAMERA_Y: i32 = 0x20;

// The restored throne room's tables (`sma4_assets::ENDING_ROM`).
const BOX_ROWS: u8 = 10;
const MESSAGE_BY_FORM: u32 = 0x0813_F6A8;
const PIECES: u32 = 0x0813_F924;
const KING_BASE: u32 = 0x0813_F940;
const KING_TILES: u32 = 0x0813_F947;
const KING_FLIPS: u32 = 0x0813_F97F;
const WAND_TILES: u32 = 0x0813_F9B7;
const WAND_FLIPS: u32 = 0x0813_F9BB;
const WAND_SPIN_MASK: u32 = 0x0813_F9C3;
const WAND_SPIN_STEP: u32 = 0x0813_F9C5;
const TOAD_TILES: u32 = 0x0813_F9C7;
const TOAD_POSE: u32 = 0x0813_F9CF;
const TOAD_TIMES: u32 = 0x0813_F9D7;
const MARIO_FALLING: u32 = 0x0813_F9DF;
const MARIO_STANDING: u32 = 0x0813_F9E7;
/// The letter: the world's item, the item's tile and palette, the Princess's sprites, the
/// stamp's tiles.
const ITEM_BY_WORLD: u32 = 0x0813_F8A8;
const ITEM_TILES: u32 = 0x0813_F8B0;
const ITEM_PALETTES: u32 = 0x0813_F8CA;
const LETTER_SPRITES: u32 = 0x0813_F8D8;
const STAMP_TILES: u32 = 0x0813_F908;

/// OAM entries the scene writes (0x03002DC8 + 8 * k): the wand 102 (0x030030F8), the
/// King 103-106, Toad 107-108.
const WAND: usize = 102;
const TOAD_TOP: usize = 108;
const TOAD_BOTTOM: usize = 107;
const OAM_ENTRIES: usize = 128;
const HIDDEN: [u16; 3] = [0xA0, 0, 0];

const NEWLINE: u8 = 0xF0;
const SKIP: u8 = 0xFE;
const END: u8 = 0xFF;

/// The restored throne room (sub-states 0x20 and 0x21, 0x0300385C 1-3).
#[derive(Debug, Clone)]
pub struct Restored {
    data: Arc<EndingAsset>,
    world: u8,
    form: u8,
    /// 0x03003774: 0x20 fading in, then 0x21.
    sub: u8,
    /// 0x0300385C.
    pub step: u8,
    /// 0x03003850.
    delay: u8,
    /// 0x03003BAC.
    rows: u8,
    /// 0x03002DB0: BLDY (black 0x10).
    pub bldy: u8,
    pub message_id: u8,
    message: Vec<u8>,
    printed: usize,
    /// Toad (object slot 0): x (+0, its byte 1 the hop's height), speed +0x10, phase
    /// +0x17, timer +0x23; 0xBBD and 0xBC1 its pose timer and index.
    toad_x: i32,
    toad_v: i8,
    toad_phase: u8,
    toad_timer: u8,
    pose_timer: u8,
    pose_index: u8,
    /// The wand (0x03007848 + 0xBB0): x, y (1/256 px), speeds 0xBB8 / 0xBBA, state 0xBBF,
    /// spin frame 0xBC0.
    wand_x: i32,
    wand_y: i32,
    wand_vx: i16,
    wand_vy: i16,
    wand_state: u8,
    wand_frame: u8,
    /// Mario's y (1/256 px, from -0x100 px down to 0x88) and pose (0x0813F9DF / F9E7).
    pub mario_y: i32,
    pub mario_pose: u8,
    oam: Vec<[u16; 3]>,
}

/// What the scene asks of the game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoredEvent {
    /// 0x08029DB0 is done: the letter, with the world's item.
    Letter { item: u8 },
}

impl Restored {
    /// Mario's x (0x60 px, +0x60 = 0x40: facing right).
    pub const MARIO_X: i32 = 0x60;

    /// The init 0x08029918 (end of sub-state 0x1F).
    pub fn new(data: Arc<EndingAsset>, castle_message: impl Fn(u8) -> Vec<u8>, world: u8, form: u8) -> Self {
        let message_id = data.rom_u16(MESSAGE_BY_FORM + form as u32 * 2) as u8;
        let message = castle_message(message_id);
        Self {
            world,
            form,
            sub: 0x20,
            step: 1,
            delay: 0x30,
            rows: 0,
            bldy: 0x10,
            message_id,
            message,
            printed: 0,
            toad_x: 0,
            toad_v: -0x10,
            toad_phase: 0,
            toad_timer: 0x17,
            pose_timer: 0,
            pose_index: 0,
            wand_x: 0x72 << 8,
            wand_y: -0x2000,
            wand_vx: 0,
            wand_vy: 0x40,
            wand_state: 0,
            wand_frame: 0,
            mario_y: -0x1_0000,
            mario_pose: data.rom_u8(MARIO_FALLING + form as u32),
            oam: vec![HIDDEN; OAM_ENTRIES],
            data,
        }
    }

    fn rom(&self, a: u32) -> u8 {
        self.data.rom_u8(a)
    }

    /// One frame; `frame` is 0x03003AE8.
    pub fn step(&mut self, frame: u32) -> Option<RestoredEvent> {
        if self.sub == 0x20 {
            // 0x080298C4: the sprites without the wand or Mario; BLDY down every other frame.
            self.draw(frame);
            if frame & 1 == 0 {
                self.bldy -= 1;
                if self.bldy == 0 {
                    self.sub = 0x21;
                }
            }
            return None;
        }
        // 0x0802A98C.
        self.delay = self.delay.saturating_sub(1);
        self.pose_timer = self.pose_timer.saturating_sub(1);
        self.oam.fill(HIDDEN);
        let ev = match self.step {
            1 => {
                self.box_row();
                None
            }
            2 => {
                self.text();
                None
            }
            _ => self.fade_out(frame),
        };
        if self.step != 4 {
            self.draw(frame);
            self.mario();
        }
        ev
    }

    /// 0x08029A40: a row of the box a frame; after 10, the message.
    fn box_row(&mut self) {
        if self.delay != 0 {
            return;
        }
        self.rows += 1;
        if self.rows >= BOX_ROWS {
            self.delay = 0x10;
            self.step = 2;
        }
    }

    /// 0x08029B44: the next byte of the message (0x080AB140); its end starts the fade.
    fn text(&mut self) {
        if self.delay != 0 {
            return;
        }
        let b = self.message.get(self.printed).copied().unwrap_or(END);
        self.printed += 1;
        if b == END {
            self.step = 3;
            self.delay = 0xFF;
            self.bldy = 0;
            return;
        }
        // A letter plays sound 0x1B; a skipped cell takes 1 frame, the rest 4.
        self.delay = if b == SKIP { 1 } else { 4 };
    }

    /// 0x08029DB0: after 0xFF frames, BLDY up a step every 4 frames; past 0x10 the letter.
    fn fade_out(&mut self, frame: u32) -> Option<RestoredEvent> {
        if self.delay != 0 || frame & 3 != 0 {
            return None;
        }
        self.bldy += 1;
        if self.bldy <= 0x10 {
            return None;
        }
        self.step = 4;
        Some(RestoredEvent::Letter { item: self.rom(ITEM_BY_WORLD + self.world as u32) })
    }

    /// 0x0802A404: the King and Toad, the King talking during the text, the wand, Toad's
    /// hops.
    fn draw(&mut self, frame: u32) {
        for k in 0..7 {
            let i = PIECES + 0x18 - 4 * k as u32;
            let (x, y, tile, a) = (self.rom(i), self.rom(i + 1), self.rom(i + 2), self.rom(i + 3));
            let e = &mut self.oam[TOAD_TOP - k];
            e[1] = (e[1] & 0xFE00) | ((x as u16).wrapping_sub(0x10) & 0x1FF);
            e[0] = (e[0] & 0xFF00) | y.wrapping_add(8) as u16;
            e[2] = (e[2] & 0xFC00) | tile as u16;
            e[2] = (e[2] & 0x03FF) | ((a as u16 >> 1) & 7) << 12 | 1 << 10;
            e[1] = (e[1] & 0x3FFF) | 0x4000;
        }
        let mut u = self.rom(KING_BASE + self.world as u32);
        if self.step == 2 && frame & 0x10 != 0 {
            u = u.wrapping_add(1);
        }
        for j in 0..4 {
            let (tile, f) = (self.rom(KING_TILES + u as u32), self.rom(KING_FLIPS + u as u32) as u16);
            let e = &mut self.oam[106 - j];
            e[2] = (e[2] & 0xFC00) | tile as u16;
            e[1] = (e[1] & !0x3000) | ((f >> 6) & 1) << 12 | (f >> 7) << 13;
            u = u.wrapping_add(2);
        }
        if self.sub != 0x20 {
            self.wand(frame);
        }
        self.toad();
    }

    /// The wand thrown back to the King (0xBBF 0: falling to y 0x71, 1: bounced up toward
    /// him, 2: in his hand).
    fn wand(&mut self, frame: u32) {
        let st = self.wand_state as u32;
        if st >= 2 {
            let e = &mut self.oam[WAND];
            e[2] = (e[2] & 0xFC00) | 0xD0;
            return;
        }
        if self.rom(WAND_SPIN_MASK + st) as u32 & frame == 0 {
            self.wand_frame = self.wand_frame.wrapping_add(self.rom(WAND_SPIN_STEP + st)) & 7;
        }
        // 0x0802A3B0.
        self.wand_y += self.wand_vy as i32 * 16;
        if self.wand_vy < 0x40 {
            self.wand_vy += 2;
        }
        self.wand_x += self.wand_vx as i32 * 16;
        let y = self.wand_y >> 8;
        if st == 0 {
            if y > 0x71 {
                // Sound 0x58.
                self.wand_vx = 0x10;
                self.wand_vy = -0x50;
                self.wand_state = 1;
            }
        } else if y > 0x47 && (self.wand_vy as i8) >= 0 {
            // Music 0x1E.
            self.wand_state = 2;
        }
        let x = (((self.wand_x >> 8) & 0xFF) as u16).wrapping_sub(0x10) & 0x1FF;
        let f = self.rom(WAND_FLIPS + self.wand_frame as u32) as u16;
        let tile = self.rom(WAND_TILES + (self.wand_frame & 3) as u32) as u16;
        let e = &mut self.oam[WAND];
        e[0] = (e[0] & 0xFF00) | ((y as u8).wrapping_add(8)) as u16;
        e[2] = 0x1400 | tile;
        e[1] = (e[1] & 0x0E00) | x | ((f >> 6) & 1) << 12 | (f >> 7) << 13 | 0x4000;
    }

    /// Toad's hops: two (phases 0-3, 7 frames apart), then a pose cycle (phase 4), then
    /// 0x17 frames' rest.
    fn toad(&mut self) {
        let mut pose = 0u8;
        let mut cycle = self.toad_phase == 4;
        if !cycle {
            let mut hop = true;
            if self.toad_phase & 1 == 0 {
                if self.toad_timer != 0 {
                    self.toad_timer -= 1;
                    hop = false;
                } else {
                    self.toad_phase += 1;
                    self.toad_v = -0x10;
                    self.toad_x &= !0xFF00;
                }
            }
            if hop {
                self.toad_v = self.toad_v.wrapping_add(1);
                // 0x08049DB8.
                self.toad_x += self.toad_v as i32 * 16;
                if (self.toad_x >> 8) as u8 != 0 {
                    pose = 6;
                } else if self.toad_phase == 3 {
                    self.toad_v = 0;
                    self.toad_x &= !0xFF;
                    self.pose_index = 0;
                    self.pose_timer = 0;
                    cycle = true;
                } else {
                    self.toad_phase += 1;
                    self.toad_timer = 7;
                }
            }
        }
        if cycle {
            let mut idx = self.pose_index;
            let mut rest = false;
            if self.pose_timer == 0 {
                idx = idx.wrapping_sub(1);
                if (idx as i8) < 0 {
                    if self.toad_phase == 4 {
                        self.toad_phase = 0;
                        self.toad_timer = 0x17;
                        rest = true;
                    } else {
                        self.toad_phase += 1;
                        idx = 7;
                    }
                }
                if !rest {
                    self.pose_index = idx;
                    self.pose_timer = self.rom(TOAD_TIMES + idx as u32);
                }
            }
            if !rest {
                pose = self.rom(TOAD_POSE + idx as u32);
            }
        }
        let h = (self.toad_x >> 8) as u8;
        let (top, bottom) = (self.rom(TOAD_TILES + pose as u32), self.rom(TOAD_TILES + 1 + pose as u32));
        let e = &mut self.oam[TOAD_TOP];
        e[2] = (e[2] & 0xFC00) | top as u16;
        e[0] = (e[0] & 0xFF00) | ((e[0] as u8).wrapping_add(h)) as u16;
        let e = &mut self.oam[TOAD_BOTTOM];
        e[2] = (e[2] & 0xFC00) | bottom as u16;
        e[0] = (e[0] & 0xFF00) | ((e[0] as u8).wrapping_add(h)) as u16;
    }

    /// 0x0802A8C0: Mario drops 4 px a frame (holding 0x03003850) until y 0x88.
    fn mario(&mut self) {
        let falling = ((self.mario_y >> 8) & 0xFF) < 0x88 || (self.mario_y >> 16) & 0xFFFF != 0;
        if falling {
            self.mario_y += 0x400;
            self.delay = self.delay.wrapping_add(1);
            self.mario_pose = self.rom(MARIO_FALLING + self.form as u32);
        } else {
            self.mario_pose = self.rom(MARIO_STANDING + self.form as u32);
        }
    }

    /// The OAM entries (front first).
    pub fn sprites(&self) -> Vec<ObjSprite> {
        self.oam.iter().filter_map(|e| ObjSprite::from_attrs(e[0], e[1], e[2])).collect()
    }

    pub fn oam(&self, index: usize) -> [u16; 3] {
        self.oam[index]
    }

    /// Box rows written (`sma4_assets::ENDING_BOX_H` when done).
    pub fn box_rows(&self) -> u8 {
        if self.step == 1 { self.rows } else if self.step <= 3 { BOX_ROWS } else { 0 }
    }

    /// The message's cells printed so far, relative to `sma4_assets::CASTLE_TEXT_AT`.
    pub fn text_cells(&self) -> Vec<(u32, u32)> {
        if self.step < 2 || self.step > 3 {
            return Vec::new();
        }
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

    /// Mario is drawn (from the scene's start).
    pub fn mario_shown(&self) -> bool {
        self.sub == 0x21 && self.step < 4
    }
}

/// The GBA's registers for the letter (BG1 and OBJ, BG1CNT 0xD801) and the save screen
/// (BG1-2 and OBJ; BG0, the status bar, is the host's map bar).
const LETTER_DISPCNT: u16 = 0x1200;
const LETTER_BGCNT: [u16; 4] = [0x9400, 0xD801, 0xDC03, 0x9608];
const CLEAR_DISPCNT: u16 = 0x1600;
const CLEAR_BGCNT: [u16; 4] = [0x5600, 0x5A02, 0x1E03, 0x9608];
/// BLDCNT 0xFF: everything darkened by BLDY.
const BLD_ALL: u16 = 0xFF;

fn video(snap: &sma4_assets::VideoSnapshot, dispcnt: u16, bgcnt: [u16; 4]) -> Video {
    let mut palette = [0u16; 512];
    for (p, &c) in palette.iter_mut().zip(&snap.palette) {
        *p = c;
    }
    Video {
        vram: snap.vram.clone(),
        palette,
        oam: [0; 0x400],
        dispcnt,
        bgcnt,
        scroll: [(0, 0); 4],
        bldcnt: BLD_ALL,
        bldalpha: 0,
        bldy: 0x10,
    }
}

fn put(oam: &mut [u8; 0x400], k: usize, e: [u16; 3]) {
    if k < 128 {
        for (j, v) in e.iter().enumerate() {
            oam[k * 8 + j * 2..k * 8 + j * 2 + 2].copy_from_slice(&v.to_le_bytes());
        }
    }
}

fn get(oam: &[u8; 0x400], k: usize) -> [u16; 3] {
    std::array::from_fn(|j| u16::from_le_bytes([oam[k * 8 + j * 2], oam[k * 8 + j * 2 + 1]]))
}

fn clear_oam(oam: &mut [u8; 0x400]) {
    for k in 0..128 {
        put(oam, k, HIDDEN);
    }
}

/// The Princess's letter (0x0300385C = 4, 0x0802A0A8): it fades in, her stamp blinks while
/// it does, the item she sends blinks once it is in; A or B ends it.
#[derive(Debug, Clone)]
pub struct Letter {
    data: Arc<EndingAsset>,
    pub item: u8,
    pub video: Video,
    /// 0x0300384A (left at 0 by the throne room).
    stamp: u8,
}

impl Letter {
    pub fn new(data: Arc<EndingAsset>, item: u8) -> Self {
        // 0x08029DB0 leaves BLDY at 0x11.
        let mut video = video(&data.letter, LETTER_DISPCNT, LETTER_BGCNT);
        video.bldy = 0x11;
        Self { data, item, video, stamp: 0 }
    }

    /// One frame (after 0x0802A98C's OAM clear); true when A or B ends it.
    pub fn step(&mut self, pressed: Buttons, frame: u32) -> bool {
        let d = &self.data;
        let v = &mut self.video;
        clear_oam(&mut v.oam);
        for k in 0..6 {
            let t = LETTER_SPRITES + k as u32 * 8;
            let (x, y, tile) = (d.rom_u16(t), d.rom_u16(t + 2), d.rom_u16(t + 4));
            let mut e = get(&v.oam, 102 + k);
            e[1] = (e[1] & 0xFE00) | (x & 0x1FF);
            e[0] = (e[0] & 0xFF00) | (y & 0xFF);
            e[2] = (e[2] & 0xFC00) | (tile & 0x3FF);
            e[2] = (e[2] & 0x03FF) | 0x5400;
            e[1] = (e[1] & 0x3FFF) | 0x4000;
            put(&mut v.oam, 102 + k, e);
        }
        if v.bldy != 0 {
            if frame & 3 != 0 {
                let mut e = get(&v.oam, 107);
                e[0] = (e[0] & 0xFF00) | 0xA0;
                put(&mut v.oam, 107, e);
                return false;
            }
            // Music 0x29.
            v.bldy -= 1;
        }
        if pressed.contains(Buttons::A) || pressed.contains(Buttons::B) {
            return true;
        }
        let mut s = 2u32;
        if v.bldy != 0 && self.stamp != 0xFF {
            s = 1;
            if frame & 0x38 == 0 {
                self.stamp = 1;
            } else {
                s = ((frame & 0x38) >> 2) & 2;
                if self.stamp != 0 {
                    self.stamp = self.stamp.wrapping_sub(2);
                }
            }
        }
        let mut e = get(&v.oam, 102);
        e[2] = (e[2] & 0xFC00) | (d.rom_u16(STAMP_TILES + s * 2) & 0x3FF);
        put(&mut v.oam, 102, e);
        let mut e = get(&v.oam, 107);
        if frame & 0x18 != 0 && v.bldy == 0 && self.item != 0 {
            let i = self.item as u32 - 1;
            let t = d.rom_u16(ITEM_TILES + i * 2);
            let pal = (d.rom_u8(ITEM_PALETTES + i) as u16 >> 1) & 7;
            e[2] = (t & 0x3FF) | pal << 12 | 1 << 10;
        } else {
            e[0] = (e[0] & 0xFF00) | 0xA0;
        }
        put(&mut v.oam, 107, e);
        false
    }
}

// The save screen's tables.
const CLOUD_Y: u32 = 0x0814_B19C;
const CLOUD_X0: u32 = 0x0814_B1A3;
const TEXT_LIST: u32 = 0x0814_B1C0;
const DIGIT_X: u32 = 0x0814_B1E8;
const DIGIT_TILES: u32 = 0x0814_B1F2;
const OPEN_LISTS: u32 = 0x0814_B254;
const CURSOR_X: u32 = 0x0814_B268;
const PROMPT_NO: u32 = 0x0814_B27C;
const PROMPT_YES: u32 = 0x0814_B290;
const SAVING_OPEN: u32 = 0x0814_B308;
const SAVING_TEXT: u32 = 0x0814_B31C;
/// "Saving..." goes to OBJ tile 0x380 (0x06017000).
const SAVING_TILES: usize = 0x1_7000;
/// Step 6 waits for sound 0xF this long before counting (0x080E7EB0, mGBA), and at 30
/// the save (0x0803E03C) holds the game this long: the frame's text and clouds are not
/// drawn meanwhile, only the window (mGBA: frames 1848-1893).
const SAVE_SOUND: u8 = 2;
const SAVE_AT: u8 = 30;
const SAVE_HOLD: u8 = 46;
/// The init (0x08039C30, 6 frames on the GBA) to the first step.
const CLEAR_LOAD: u8 = 6;

/// "WORLD n CLEAR!" and the save prompt (sub-states 0x48 / 0x49): the castle with its
/// flag, clouds drifting by, the window asking to save; Yes shows "Saving...". The port
/// keeps no saves.
#[derive(Debug, Clone)]
pub struct WorldClear {
    data: Arc<EndingAsset>,
    /// The world just cleared (0x030075F1 >> 5).
    pub world: u8,
    pub video: Video,
    load: u8,
    /// 0x03003A57 (step), 0x03003A56 (timer), 0x03003A52 (window size << 2 | cursor),
    /// 0x03003A55 (flag frame).
    step: u8,
    timer: u8,
    window: u8,
    flag: u8,
    sound: u8,
    hold: u8,
    /// Clouds: 0x03003B7E / 0x03003B85 (x low / high), 0x03003B8C (kinds).
    cloud_lo: [u8; 7],
    cloud_hi: [u8; 7],
    cloud_kind: u8,
    /// 0x03003781: the next OAM entry.
    next: usize,
}

impl WorldClear {
    pub fn new(data: Arc<EndingAsset>, world: u8, frame: u32) -> Self {
        let snap = data.clear.first().cloned().unwrap_or_default();
        let mut v = video(&snap, CLEAR_DISPCNT, CLEAR_BGCNT);
        if v.vram.len() < sma4_assets::ENDING_VRAM {
            v.vram.resize(sma4_assets::ENDING_VRAM, 0);
        }
        let mut c = Self {
            world,
            video: v,
            load: CLEAR_LOAD,
            step: 0,
            timer: 0x1E,
            window: 0,
            flag: 0,
            sound: SAVE_SOUND,
            hold: 0,
            cloud_lo: [0; 7],
            cloud_hi: [0; 7],
            cloud_kind: 0,
            next: 4,
            data,
        };
        // 0x08039798.
        for i in 0..7 {
            c.cloud_lo[i] = c.data.rom_u8(CLOUD_X0 + i as u32);
            c.cloud_kind |= (cloud_kind(i as u32, frame) & 1) << i;
        }
        c
    }

    fn list(&mut self, ptr: u32, y: u8, x: u16) {
        let d = &self.data;
        let mut p = ptr;
        while d.rom_u16(p + 6) != 0xFFFF {
            let (b0, b1) = (d.rom_u8(p), d.rom_u8(p + 1));
            let a1 = d.rom_u16(p + 2);
            let a0 = (b0.wrapping_add(y)) as u16 | (b1 as u16) << 8;
            let a1 = ((a1 & 0x1FF).wrapping_add(x) & 0x1FF) | (a1 & 0xFE00);
            put(&mut self.video.oam, self.next, [a0, a1, d.rom_u16(p + 4)]);
            self.next += 1;
            p += 8;
        }
    }

    fn lang_list(&mut self, table: u32) {
        let p = self.data.rom_u32(table);
        self.list(p, 0x50, 0x78);
    }

    fn sized_list(&mut self, table: u32) {
        let t = self.data.rom_u32(table);
        let p = self.data.rom_u32(t + (self.window as u32 & 0x3C));
        self.list(p, 0x50, 0x78);
    }

    fn size(&self) -> u8 {
        (self.window & 0x3C) >> 2
    }

    fn set_size(&mut self, s: u8) {
        self.window = (self.window & 0xC3) | (s & 0xF) << 2;
    }

    /// One frame (0x0803AE48); true once it is over (sub-state 0x0A: the next map).
    pub fn step(&mut self, pressed: Buttons, frame: u32) -> bool {
        if self.load > 0 {
            self.load -= 1;
            return false;
        }
        clear_oam(&mut self.video.oam);
        self.next = 4;
        self.flag_frame(frame);
        let yes = self.window & 3 == 0;
        match self.step {
            0 => {
                self.timer -= 1;
                if self.timer == 0 {
                    self.step = 1;
                }
            }
            1 => {
                if frame & 1 == 0 {
                    self.video.bldy -= 1;
                    if self.video.bldy == 0 {
                        self.timer = 0;
                        self.set_size(0);
                        self.step = 2;
                    }
                }
            }
            2 => {
                // Sound 0x10 as it opens.
                self.timer += 1;
                if self.timer > 1 {
                    if self.size() < 3 {
                        self.timer = 0;
                        self.set_size(self.size() + 1);
                    } else {
                        self.step = 3;
                    }
                }
                self.sized_list(OPEN_LISTS);
            }
            3 => {
                if pressed.contains(Buttons::A) || pressed.contains(Buttons::START) {
                    // Yes: sound 0xF, else 0xE.
                    self.timer = 0;
                    self.step = 4;
                } else if pressed.contains(Buttons::LEFT) || pressed.contains(Buttons::RIGHT) {
                    let before = self.window & 3;
                    if pressed.contains(Buttons::LEFT) {
                        if before != 0 {
                            self.window = (self.window & 0xFC) | (before - 1);
                        }
                    } else if before == 0 {
                        self.window = (self.window & 0xFC) | 1;
                    }
                    if before != self.window & 3 {
                        // Sound 0x45.
                        self.timer = 0;
                    }
                }
                // 0x0803A5F4: the cursor, blinking.
                if self.timer & 0x10 == 0 {
                    let x = self.data.rom_u16(CURSOR_X + (self.window as u32 & 3) * 2) & 0x1FF;
                    put(&mut self.video.oam, self.next, [0x8054, x, 0x439E]);
                    self.next += 1;
                }
                self.timer = self.timer.wrapping_add(1);
                self.lang_list(if self.window & 3 == 0 { PROMPT_YES } else { PROMPT_NO });
            }
            4 => {
                self.timer += 1;
                if self.timer > 1 {
                    self.timer = 0;
                    if self.size() == 0 {
                        if yes {
                            let n = self.data.saving.len().min(sma4_assets::ENDING_VRAM - SAVING_TILES);
                            self.video.vram[SAVING_TILES..SAVING_TILES + n].copy_from_slice(&self.data.saving[..n]);
                            self.step = 5;
                        } else {
                            self.step = 11;
                        }
                    } else {
                        self.set_size(self.size() - 1);
                    }
                }
                self.sized_list(OPEN_LISTS);
            }
            5 => {
                self.timer += 1;
                if self.timer > 1 {
                    self.timer = 0;
                    if self.size() < 3 {
                        self.set_size(self.size() + 1);
                    } else {
                        self.step = 6;
                    }
                }
                self.sized_list(SAVING_OPEN);
            }
            6 => {
                self.lang_list(SAVING_TEXT);
                self.sized_list(SAVING_OPEN);
                // Waits for sound 0xF (0x080E7EB0), counts to 60; the save at 30 (the port
                // keeps no saves, but the game holds as long).
                if self.hold > 0 {
                    self.hold -= 1;
                    if self.hold > 0 {
                        return false;
                    }
                } else if self.sound > 0 {
                    self.sound -= 1;
                } else {
                    self.timer += 1;
                    if self.timer == SAVE_AT {
                        self.hold = SAVE_HOLD;
                        return false;
                    }
                    if self.timer == 60 {
                        self.timer = 0;
                        self.step = 7;
                    }
                }
            }
            7 => {
                self.sized_list(SAVING_OPEN);
                self.timer += 1;
                if self.timer > 1 {
                    self.timer = 0;
                    if self.size() == 0 {
                        self.step = 11;
                    } else {
                        self.set_size(self.size() - 1);
                    }
                }
            }
            _ => return true,
        }
        self.text();
        self.clouds(frame);
        false
    }

    /// 0x0803A024: the flag on the castle, three frames, every 8 frames (BG1's map).
    fn flag_frame(&mut self, frame: u32) {
        if frame & 7 != 0 {
            return;
        }
        self.flag = if self.flag != 2 { self.flag + 1 } else { 0 };
        let map = (CLEAR_BGCNT[1] as usize & 0x1F00) * 8;
        for k in 0..3u16 {
            for (at, base) in [(0xEE, 0x2200), (0x12E, 0x2203)] {
                let v = self.flag as u16 * 6 + base + k;
                let o = map + at + k as usize * 2;
                self.video.vram[o..o + 2].copy_from_slice(&v.to_le_bytes());
            }
        }
    }

    /// 0x0803A344: "WORLD n CLEAR!" and the world's number.
    fn text(&mut self) {
        self.lang_list(TEXT_LIST);
        let x = (self.data.rom_u16(DIGIT_X).wrapping_add(0x78)) & 0x1FF;
        let tile = self.data.rom_u16(DIGIT_TILES + self.world as u32 * 2) & 0x3FF;
        put(&mut self.video.oam, self.next, [0x0004, x | 0x4000, tile | 0x1000]);
        self.next += 1;
    }

    /// 0x080397EC: seven clouds drifting right, 32x16 OBJs behind everything.
    fn clouds(&mut self, frame: u32) {
        for i in 0..7usize {
            if frame & 3 == 0 {
                let before = self.cloud_lo[i];
                let step = if self.cloud_kind >> i & 1 == 0 { 1 } else { ((frame & 4) >> 2) as u8 };
                self.cloud_lo[i] = before.wrapping_add(step);
                let now = self.cloud_lo[i];
                if before == now || now != 0 {
                    if self.cloud_hi[i] == 1 && now == 0x20 {
                        self.cloud_hi[i] = 0;
                        self.cloud_lo[i] = 0;
                        self.cloud_kind ^= (cloud_kind(i as u32, frame) & 1) << i;
                    }
                } else {
                    self.cloud_hi[i] += 1;
                }
            }
            let x = ((self.cloud_hi[i] as u16) << 8 | self.cloud_lo[i] as u16).wrapping_sub(0x30) & 0x1FF;
            let tile = if self.cloud_kind >> i & 1 == 0 { 600 } else { 0x25C };
            let y = self.data.rom_u8(CLOUD_Y + i as u32) as u16;
            put(&mut self.video.oam, self.next, [y | 0x4000, x | 0x8000, tile | 0xEC00]);
            self.next += 1;
        }
    }

    /// The window is shown (for the host's status bar, which stays).
    pub fn loaded(&self) -> bool {
        self.load == 0
    }
}

/// 0x0803975C: (i + ((frame + VCOUNT) ^ frame ^ 0x03003CF6)) & 0xFF, bit 0 picks the
/// cloud. TODO(port): VCOUNT (the scanline the code runs on) cannot be known here; the
/// port uses the frame counter's low bit instead.
fn cloud_kind(i: u32, frame: u32) -> u8 {
    (i + (frame & 1)) as u8
}

/// Where the end of a castle is.
#[derive(Debug, Clone)]
pub enum Ending {
    /// The level is over (frames since): the fade, black, the throne room loads.
    Leaving(u16),
    Throne(Restored),
    Letter(Letter),
    /// Sub-state 0x22 (0x0802AA34): the palette fades out (frames since).
    Closing(u8),
    Clear(WorldClear),
}

/// Sub-state 0x22's fade: 15 frames (mGBA).
pub const CLOSING: u8 = 15;

impl Ending {
    /// How dark the screen is (BLDY or the palette fade, 0-16) over the level's scene.
    pub fn darkness(&self) -> u8 {
        match self {
            Ending::Leaving(t) => {
                if *t >= FADE_OUT { 16 } else { (*t as u32 * 16 / FADE_OUT as u32) as u8 }
            }
            Ending::Throne(r) => r.bldy.min(16),
            Ending::Closing(t) => (*t as u32 * 16 / CLOSING as u32).min(16) as u8,
            _ => 0,
        }
    }

    /// The scene shows as the GBA's video memory (`Video`) instead of the level.
    pub fn video(&self) -> Option<&Video> {
        match self {
            Ending::Letter(l) => Some(&l.video),
            Ending::Clear(c) => Some(&c.video),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset() -> Arc<EndingAsset> {
        Arc::new(EndingAsset::default())
    }

    #[test]
    fn mario_drops_in_holding_the_box_then_it_opens_and_the_text_prints() {
        let mut r = Restored::new(asset(), |_| vec![1, 2, SKIP, 3, NEWLINE, 4, END], 0, 0);
        let mut f = 0u32;
        while r.sub == 0x20 {
            f += 1;
            r.step(f);
        }
        assert_eq!(f, 32, "BLDY 16 down every other frame");
        let mut landed = None;
        let mut box_at = None;
        while r.step < 3 {
            f += 1;
            r.step(f);
            if landed.is_none() && r.mario_y == 0x8800 {
                landed = Some(f);
            }
            if box_at.is_none() && r.rows > 0 {
                box_at = Some(f);
            }
            assert!(f < 1000);
        }
        // mGBA (frames after the level's end): 0x21 at 96, Mario at y 0x88 at 194 (98
        // frames of 4 px from -0x100), the first box row at 242.
        let (l, b) = (landed.unwrap(), box_at.unwrap());
        assert_eq!(l - 32, 194 - 96);
        assert_eq!(b - l, 242 - 194);
        assert_eq!(r.text_cells(), vec![(0, 0), (1, 0), (3, 0), (0, 1)]);
    }

    #[test]
    fn the_letter_fades_in_over_68_frames_and_a_ends_it() {
        let mut l = Letter::new(asset(), 8);
        let mut f = 0;
        while l.video.bldy != 0 {
            f += 1;
            assert!(!l.step(Buttons::empty(), f));
        }
        assert_eq!(f, 68);
        assert!(l.step(Buttons::A, f + 1));
    }
}

