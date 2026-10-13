//! Renders object sprites the way the game builds them: run the object's own draw routine
//! on a fake object slot, read back the OAM entries it wrote, and compose those from OBJ
//! VRAM. Used for enemies made of several hardware sprites (e.g. the Koopa Troopa).

use sma4_assets::Rgba;
use sma4_rom::gfx::{Bgr555, parse_palette};

use crate::emu::Machine;
use crate::level::ExtractError;

const OBJ_VRAM: u32 = 0x0601_0000;
const OBJ_PAL_BUFFER: u32 = 0x0200_0200;
/// OAM shadow copied to OAM in VBlank (0x08001E14).
const OAM_BUF: u32 = 0x0300_2DC8;
const OBJ_BASE_PTR: u32 = 0x0300_7848;
const PLAYER_PTR: u32 = 0x0300_7818;
const PLAYER_GFX_PTR: u32 = 0x0300_7820;
const CAMERA_X: u32 = 0x0300_3750;
const CAMERA_Y: u32 = 0x0300_3CE0;
/// Scratch object slots (0x70 bytes); the drawn object is slot 0.
const OBJS: u32 = 0x0203_8000;
const SCRATCH_PLAYER: u32 = 0x0203_A000;
const SCRATCH_PLAYER_GFX: u32 = 0x0203_B000;
/// OAM index handed to the object (0x03007848 + 0xC96 holds it * 4); routines also use the
/// few entries before it.
const SLOT: u32 = 16;
const SLOTS_BEFORE: u32 = 8;
const SLOTS_AFTER: u32 = 16;
/// Second OAM range some routines draw into (0x030030F8 = entry 102, e.g. the goal box
/// 0x080675D0, indexed by 0x03007848 + 0xCCD = 0).
const LOW_SLOTS: std::ops::Range<u32> = 102..118;
/// Off-screen y the game uses for unused entries.
const HIDDEN_Y: u16 = 0xA0;
/// Object position on screen while drawing.
pub const AT: i32 = 64;
const PALETTES: u32 = 0x0816_A770;

/// Cell size of the composed frames and where the object's position lands in it.
pub const CELL_W: u32 = 32;
pub const CELL_H: u32 = 48;
pub const ORIGIN: (i32, i32) = (8, 24);

/// One object state to render: the draw routine and the slot bytes / globals it reads.
pub struct Pose<'a> {
    pub kind: u16,
    pub state: u8,
    pub draw: u32,
    pub bytes: &'a [(u32, u8)],
    pub globals: &'a [(u32, u8)],
}

struct Obj {
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    tile: u32,
    pal: u32,
    hflip: bool,
    vflip: bool,
}

/// OBJ sizes by shape (square, wide, tall) and size.
const SIZES: [[(u32, u32); 4]; 3] = [
    [(8, 8), (16, 16), (32, 32), (64, 64)],
    [(16, 8), (32, 8), (32, 16), (64, 32)],
    [(8, 16), (8, 32), (16, 32), (32, 64)],
];

fn capture(m: &mut Machine, rom: &[u8], pose: &Pose) -> Result<Vec<Obj>, ExtractError> {
    m.w32(OBJ_BASE_PTR, OBJS)?;
    m.w32(PLAYER_PTR, SCRATCH_PLAYER)?;
    m.w32(PLAYER_GFX_PTR, SCRATCH_PLAYER_GFX)?;
    m.w16(CAMERA_X, 0)?;
    m.w16(CAMERA_Y, 0)?;
    for i in 0..0x70 {
        m.w8(OBJS + i, 0)?;
    }
    let first = SLOT - SLOTS_BEFORE;
    let slots = || (first..SLOT + SLOTS_AFTER).chain(LOW_SLOTS);
    for i in slots() {
        m.w16(OAM_BUF + i * 8, HIDDEN_Y)?;
        m.w16(OAM_BUF + i * 8 + 2, 0)?;
        m.w16(OAM_BUF + i * 8 + 4, 0)?;
    }
    m.w32(OBJS, (AT as u32) << 8)?;
    m.w32(OBJS + 8, (AT as u32) << 8)?;
    m.w16(OBJS + 0x18, pose.kind)?;
    m.w8(OBJS + 0x16, pose.state)?;
    m.w8(OBJS + 0x21, rom[(PALETTES - 0x0800_0000) as usize + pose.kind as usize] & 7)?;
    m.w8(OBJS + 0xBD0, 0)?;
    m.w8(OBJS + 0xCCD, 0)?;
    m.w16(OBJS + 0xC96, (SLOT * 4) as u16)?;
    for &(off, v) in pose.bytes {
        m.w8(OBJS + off, v)?;
    }
    for &(addr, v) in pose.globals {
        m.w8(addr, v)?;
    }
    m.call(pose.draw, &[OBJS])?;

    let mut objs = Vec::new();
    // Lower OAM indices are drawn on top: collect back to front.
    for i in slots().collect::<Vec<_>>().into_iter().rev() {
        let e = m.read(OAM_BUF + i * 8, 6)?;
        let a0 = u16::from_le_bytes([e[0], e[1]]);
        let a1 = u16::from_le_bytes([e[2], e[3]]);
        let a2 = u16::from_le_bytes([e[4], e[5]]);
        if a0 & 0xFF == HIDDEN_Y || a0 & 0x0200 != 0 {
            continue;
        }
        let shape = (a0 >> 14) as usize;
        if shape > 2 {
            continue;
        }
        let (w, h) = SIZES[shape][(a1 >> 14) as usize];
        // Signed screen coordinates (y wraps at 256, x at 512).
        let y = (a0 & 0xFF) as i32;
        let x = (a1 & 0x1FF) as i32;
        objs.push(Obj {
            x: if x >= 256 { x - 512 } else { x },
            y: if y >= 160 + 64 { y - 256 } else { y },
            w,
            h,
            tile: (a2 & 0x3FF) as u32,
            pal: (a2 >> 12) as u32,
            hflip: a1 & 0x1000 != 0,
            vflip: a1 & 0x2000 != 0,
        });
    }
    Ok(objs)
}

fn compose(objs: &[Obj], chars: &[u8], palette: &[Bgr555]) -> Rgba {
    let mut px = vec![0u8; (CELL_W * CELL_H * 4) as usize];
    for o in objs {
        for sy in 0..o.h {
            for sx in 0..o.w {
                // 2D mapping: 32 tiles per row.
                let (tx, ty) = (sx / 8, sy / 8);
                let tile = o.tile + ty * 32 + tx;
                let t = &chars[(tile as usize * 32) % chars.len()..][..32];
                let b = t[((sy % 8) * 4 + (sx % 8) / 2) as usize];
                let ci = if sx & 1 == 1 { b >> 4 } else { b & 0xF };
                if ci == 0 {
                    continue;
                }
                let dx = if o.hflip { o.w - 1 - sx } else { sx } as i32;
                let dy = if o.vflip { o.h - 1 - sy } else { sy } as i32;
                let cx = o.x + dx - AT + ORIGIN.0;
                let cy = o.y + dy - AT + ORIGIN.1;
                if !(0..CELL_W as i32).contains(&cx) || !(0..CELL_H as i32).contains(&cy) {
                    continue;
                }
                let c = palette[(o.pal * 16 + ci as u32) as usize].to_rgba(255);
                let i = ((cy as u32 * CELL_W + cx as u32) * 4) as usize;
                px[i..i + 4].copy_from_slice(&c);
            }
        }
    }
    Rgba { width: CELL_W, height: CELL_H, pixels: px }
}

/// Renders the poses side by side, one `CELL_W` x `CELL_H` cell each.
pub fn render(m: &mut Machine, rom: &[u8], poses: &[Pose]) -> Result<Rgba, ExtractError> {
    let chars = m.read(OBJ_VRAM, 0x8000)?;
    let palette = parse_palette(&m.read(OBJ_PAL_BUFFER, 0x200)?);
    let w = CELL_W * poses.len() as u32;
    let mut sheet = vec![0u8; (w * CELL_H * 4) as usize];
    for (i, pose) in poses.iter().enumerate() {
        let cell = compose(&capture(m, rom, pose)?, &chars, &palette);
        for y in 0..CELL_H {
            let src = (y * CELL_W * 4) as usize;
            let dst = ((y * w + i as u32 * CELL_W) * 4) as usize;
            sheet[dst..dst + (CELL_W * 4) as usize].copy_from_slice(&cell.pixels[src..src + (CELL_W * 4) as usize]);
        }
    }
    Ok(Rgba { width: w, height: CELL_H, pixels: sheet })
}
