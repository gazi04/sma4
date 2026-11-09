//! The King's castle and the airship (`castle/`): the throne room's tables, messages and
//! message box, and OBJ VRAM snapshots of the rooms whose objects the game draws as
//! hardware sprites. See docs/re-notes.md "King's castle".

use sma4_assets::{CASTLE_BOX_H, CASTLE_BOX_W, CASTLE_TEXT_AT, CastleAsset, ObjBank, Rgba};
use sma4_rom::Rom;
use sma4_rom::gfx::parse_palette;

use crate::emu::Machine;
use crate::level::ExtractError;
use crate::objects::Sheet;

const OBJ_VRAM: u32 = 0x0601_0000;
const OBJ_PAL_BUFFER: u32 = 0x0200_0200;
const BG_PAL_BUFFER: u32 = 0x0200_0000;
/// BG3's characters (BG3CNT 0x9608: char block 2).
const BG3_CHARS: u32 = 0x0600_8000;
/// Box rows (0x08093EA0): 0x1A map entries from 0x08171EC0[row].
const BOX_ROWS: u32 = 0x0817_1EC0;
/// The language's text table (0x080AB0CC).
const TEXT_TABLES: u32 = 0x0824_4318;
const LANG: u32 = 0;

fn err(e: impl std::fmt::Display) -> ExtractError {
    ExtractError(e.to_string())
}

/// The OBJ VRAM and palette as the room just extracted (`level::extract`) left them.
pub fn write_bank(m: &Machine, out_dir: &std::path::Path, level: u16) -> Result<(), ExtractError> {
    let bank = ObjBank { vram: m.read(OBJ_VRAM, 0x8000)?, palette: m.read(OBJ_PAL_BUFFER, 0x200)? };
    bank.write(out_dir, level).map_err(err)
}

fn message(rom: &Rom, id: u8) -> Result<Vec<u8>, ExtractError> {
    let mut p = rom.ptr(rom.ptr(TEXT_TABLES + LANG * 4)? + id as u32 * 4)?;
    let mut bytes = Vec::new();
    loop {
        let b = rom.u8(p)?;
        p += 1;
        bytes.push(b);
        if b == 0xFF {
            return Ok(bytes);
        }
    }
}

/// The throne room (level 0x1C5 extracted): its tables, messages, box and OBJ bank.
pub fn extract_throne(rom: &Rom, m: &mut Machine, out_dir: &std::path::Path, level: u16) -> Result<(), ExtractError> {
    let mut asset = CastleAsset::default();
    for &(a, b) in sma4_assets::CASTLE_ROM {
        asset.rom.push((a, rom.slice(a, (b - a) as usize)?.to_vec()));
    }
    for id in sma4_assets::CASTLE_MESSAGES {
        asset.messages.push((id, message(rom, id)?));
    }
    asset.write(out_dir).map_err(err)?;

    let chars = m.read(BG3_CHARS, 0x8000)?;
    let sheet = Sheet { chars: &chars, palette: parse_palette(&m.read(BG_PAL_BUFFER, 0x200)?) };
    let (w, h) = (CASTLE_BOX_W * 8, CASTLE_BOX_H * 8);
    let entry = |px: &mut Vec<u8>, e: u16, tx: u32, ty: u32| {
        sheet.tile_flip(px, w, (e & 0x3FF) as u32, tx * 8, ty * 8, e & 0x400 != 0, e & 0x800 != 0, (e >> 12) as u32);
    };
    let mut boxed = vec![0u8; (w * h * 4) as usize];
    for ty in 0..CASTLE_BOX_H {
        let row = rom.ptr(BOX_ROWS + ty * 4)?;
        for tx in 0..CASTLE_BOX_W {
            entry(&mut boxed, rom.u16(row + tx * 2)?, tx, ty);
        }
    }
    let png = |px: Vec<u8>, path: std::path::PathBuf| Rgba { width: w, height: h, pixels: px }.write_png(&path).map_err(err);
    png(boxed.clone(), sma4_assets::castle_box_path(out_dir))?;
    for (id, bytes) in &asset.messages {
        let mut px = boxed.clone();
        let (mut line, mut col) = (0, 0);
        for &b in bytes {
            match b {
                0xFF => break,
                0xF0 => (line, col) = (line + 1, 0),
                0xFE => col += 1,
                _ => {
                    let (tx, ty) = (CASTLE_TEXT_AT.0 + col, CASTLE_TEXT_AT.1 + line);
                    if tx < CASTLE_BOX_W && ty < CASTLE_BOX_H {
                        for y in 0..8 {
                            let o = (((ty * 8 + y) * w + tx * 8) * 4) as usize;
                            px[o..o + 32].fill(0);
                        }
                        entry(&mut px, b as u16, tx, ty);
                    }
                    col += 1;
                }
            }
        }
        png(px, sma4_assets::castle_text_path(out_dir, *id))?;
    }
    write_bank(m, out_dir, level)?;
    println!("  throne room: messages {:?}, box, sprites", sma4_assets::CASTLE_MESSAGES);
    Ok(())
}

/// "MARIO START!" (0x080244F4, from the area switch 0x0800515C into the airship's intro):
/// unpacks the language's text into OBJ tiles 0x390-0x39B.
const FN_MARIO_START: u32 = 0x0802_44F4;
/// The language (0x03003777) and the character (0x03002D77, 0 Mario).
const LANGUAGE: u32 = 0x0300_3777;
const CHARACTER: u32 = 0x0300_2D77;

/// The airship's intro (level 0x202 extracted): its OBJ bank with "MARIO START!".
pub fn extract_intro(m: &mut Machine, out_dir: &std::path::Path, level: u16) -> Result<(), ExtractError> {
    m.w8(LANGUAGE, 0)?;
    m.w8(CHARACTER, 0)?;
    m.call(FN_MARIO_START, &[])?;
    write_bank(m, out_dir, level)?;
    println!("  airship intro: sprites, MARIO START!");
    Ok(())
}

/// Larry Koopa's init (0x08071C60) loads OBJ palette 15 (0x0821E3C0 through the colour
/// curve, the first four colours by world from 0x0816F5D4); the wand caught at the end
/// of his cabin flashes in it (0x080A83EC). Run it on a scratch object for World 1 and
/// put back the globals it sets (0x03003782 bit 1, 0x03003BBD).
const FN_LARRY_INIT: u32 = 0x0807_1C60;
const WORLD: u32 = 0x0300_2A69;
const LARRY_SCRATCH: u32 = 0x0203_E000;
const LARRY_GLOBALS: [u32; 2] = [0x0300_3782, 0x0300_3BBD];

/// The colour curve (0x08007B5C(colour, 1)) and the colours Larry's palette 12 flashes to
/// when a fireball hits him (0x08072954: 0x0821F300 + 0x0816F830[world] * 2, World 1: + 0).
const FN_COLOUR_CURVE: u32 = 0x0800_7B5C;
const LARRY_FLASH: u32 = 0x0821_F300;

/// Larry's cabin (level 0x213 extracted): its OBJ bank with Larry's palette, and his flash
/// palette (`sma4_assets::larry_flash_path`).
pub fn extract_cabin(rom: &Rom, m: &mut Machine, out_dir: &std::path::Path, level: u16) -> Result<(), ExtractError> {
    let mut flash = Vec::with_capacity(32);
    for i in 0..16 {
        m.call(FN_COLOUR_CURVE, &[rom.u16(LARRY_FLASH + i * 2)? as u32, 1])?;
        flash.extend_from_slice(&(m.r0()? as u16).to_le_bytes());
    }
    std::fs::write(sma4_assets::larry_flash_path(out_dir), flash).map_err(err)?;
    let kept = LARRY_GLOBALS.map(|a| m.read(a, 1).map(|b| b[0]));
    let world = m.read(WORLD, 1)?[0];
    m.w8(WORLD, 0)?;
    for i in 0..0x70 {
        m.w8(LARRY_SCRATCH + i, 0)?;
    }
    m.call(FN_LARRY_INIT, &[LARRY_SCRATCH])?;
    m.w8(WORLD, world)?;
    for (a, v) in LARRY_GLOBALS.into_iter().zip(kept) {
        m.w8(a, v?)?;
    }
    write_bank(m, out_dir, level)?;
    println!("  Larry's cabin: sprites, Larry's palettes");
    Ok(())
}

/// The airship's sky (level 0x205: scene 2, scene mode 3). The scene's loader 0x080247C4
/// unpacks LZ77 0x08229378 (512x256) for BG2 over char block 2 (the dark clouds); the mode's
/// init 0x08025E10 unpacks LZ77 0x0823A558 (512x256) for BG3 over char block 0 (the white
/// clouds in front, priority 0). Their tiles come with the level's graphics.
const SKY_BACK: u32 = 0x0822_9378;
const SKY_FRONT: u32 = 0x0823_A558;

pub fn extract_sky(rom: &Rom, m: &Machine, out_dir: &std::path::Path) -> Result<(), ExtractError> {
    let palette = parse_palette(&m.read(BG_PAL_BUFFER, 0x200)?);
    for (stream, chars_at, path) in [
        (SKY_BACK, 0x0600_8000u32, sma4_assets::sky_path(out_dir, false)),
        (SKY_FRONT, 0x0600_0000, sma4_assets::sky_path(out_dir, true)),
    ] {
        let map = sma4_rom::decompress::lz77(rom.slice(stream, 0x4000).map_err(err)?).map_err(err)?;
        let chars = m.read(chars_at, 0x8000)?;
        let sheet = Sheet { chars: &chars, palette: palette.clone() };
        let (w, h) = (512u32, 256u32);
        let mut px = vec![0u8; (w * h * 4) as usize];
        for i in 0..(map.len() / 2).min(0x800) {
            let e = u16::from_le_bytes([map[i * 2], map[i * 2 + 1]]);
            let (block, cell) = (i as u32 / 0x400, i as u32 % 0x400);
            let (tx, ty) = (block * 32 + cell % 32, cell / 32);
            sheet.tile_flip(&mut px, w, (e & 0x3FF) as u32, tx * 8, ty * 8, e & 0x400 != 0, e & 0x800 != 0, (e >> 12) as u32);
        }
        // The back layer is opaque: its colour 0 is the backdrop (palette 0, colour 0).
        if chars_at != 0x0600_0000 {
            let bd = palette[0].to_rgba(255);
            for p in px.chunks_exact_mut(4) {
                if p[3] == 0 {
                    p.copy_from_slice(&bd);
                }
            }
        }
        Rgba { width: w, height: h, pixels: px }.write_png(&path).map_err(err)?;
    }
    println!("  airship sky");
    Ok(())
}
