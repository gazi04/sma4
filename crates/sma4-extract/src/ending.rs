//! After a world's castle (`ending/`): the restored throne room's message box, the
//! Princess's letter and the "WORLD n CLEAR!" save screen, taken from the video memory the
//! ROM's own loaders leave. See docs/re-notes.md "After the castle".

use sma4_assets::{CASTLE_BOX_W, CASTLE_TEXT_AT, ENDING_BOX_H, ENDING_VRAM, EndingAsset, Rgba, VideoSnapshot};
use sma4_rom::Rom;
use sma4_rom::gfx::parse_palette;

use crate::emu::Machine;
use crate::level::ExtractError;
use crate::objects::Sheet;

fn err(e: impl std::fmt::Display) -> ExtractError {
    ExtractError(e.to_string())
}

const VRAM: u32 = 0x0600_0000;
const PAL_BUFFER: u32 = 0x0200_0000;
const BG3_CHARS: u32 = 0x0600_8000;
const WORLD: u32 = 0x0300_2A69;
const LANGUAGE: u32 = 0x0300_3777;
const CHARACTER: u32 = 0x0300_2D77;
/// BG1CNT's shadow; the letter's BG (0x0802F214 reads its char and screen bases).
const BG1_CNT: u32 = 0x0300_2DA6;
const LETTER_BG1_CNT: u16 = 0xD801;
/// 0x08007BD0: the OBJ palettes; 0x0802F214: the letter's chars (LZ77 0x082640EC,
/// 0x0820FCAC), BG palettes 8-9 (0x08264464) and its map (0x08141F08[world]).
const FN_OBJ_PALETTES: u32 = 0x0800_7BD0;
const FN_LETTER: u32 = 0x0802_F214;
/// 0x080297E4's loads: (LZ77 stream, [(copy from, to BG1 char base +, 0x080EAA84 count)]).
const FN_LZ77: u32 = 0x080E_AAA0;
const FN_COPY: u32 = 0x080E_AA84;
const UNPACK: u32 = 0x0202_6000;
const FONT_LOADS: &[(u32, &[(u32, u32, u32)])] = &[
    (0x0818_345C, &[(0x0202_9000, 0x1000, 0x400)]),
    (0x0817_F778, &[(0x0202_6800, 0x2800, 0x200)]),
    (0x0820_FCAC, &[(0x0202_9800, 0x2000, 0x200), (0x0202_6800, 0x1400, 0x200)]),
];
/// The restored throne room's box rows (0x08029A40) and the message by form (0x0813F6A8).
const BOX_ROWS: u32 = 0x0813_F680;
const MESSAGE_BY_FORM: u32 = 0x0813_F6A8;
/// The language's text table (0x080AB0CC).
const TEXT_TABLES: u32 = 0x0824_4318;

/// The save screen's init (mode 1 sub-state 0x48): 0x030075F1 >> 5 is the world just
/// cleared.
const FN_WORLD_CLEAR: u32 = 0x0803_9C30;
const CLEARED_WORLD: u32 = 0x0300_75F1;
const SUB_STATE: u32 = 0x0300_3774;
/// "Saving..." (0x0803A870: LZ77 0x0814B2A4[language] to OBJ tile 0x380).
const SAVING_LZ: u32 = 0x0814_B2A4;

fn message(rom: &Rom, id: u8) -> Result<Vec<u8>, ExtractError> {
    let mut p = rom.ptr(rom.ptr(TEXT_TABLES)? + id as u32 * 4)?;
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

/// With the throne room (level 0x1C5) just extracted: the restored room's box and its
/// messages, then the letter's video memory.
pub fn extract_letter(rom: &Rom, m: &mut Machine, out_dir: &std::path::Path) -> Result<VideoSnapshot, ExtractError> {
    // The box over BG3, as for the throne room (`castle::extract_throne`), 10 rows.
    let chars = m.read(BG3_CHARS, 0x8000)?;
    let sheet = Sheet { chars: &chars, palette: parse_palette(&m.read(PAL_BUFFER, 0x200)?) };
    let (w, h) = (CASTLE_BOX_W * 8, ENDING_BOX_H * 8);
    let entry = |px: &mut Vec<u8>, e: u16, tx: u32, ty: u32| {
        sheet.tile_flip(px, w, (e & 0x3FF) as u32, tx * 8, ty * 8, e & 0x400 != 0, e & 0x800 != 0, (e >> 12) as u32);
    };
    let mut boxed = vec![0u8; (w * h * 4) as usize];
    for ty in 0..ENDING_BOX_H {
        let row = rom.ptr(BOX_ROWS + ty * 4)?;
        for tx in 0..CASTLE_BOX_W {
            entry(&mut boxed, rom.u16(row + tx * 2)?, tx, ty);
        }
    }
    let png = |px: Vec<u8>, path: std::path::PathBuf| Rgba { width: w, height: h, pixels: px }.write_png(&path).map_err(err);
    png(boxed.clone(), sma4_assets::ending_box_path(out_dir))?;
    let mut ids: Vec<u8> = (0..8).map(|f| rom.u8(MESSAGE_BY_FORM + f * 2)).collect::<Result<_, _>>()?;
    ids.sort_unstable();
    ids.dedup();
    for &id in &ids {
        let mut px = boxed.clone();
        let (mut line, mut col) = (0, 0);
        for b in message(rom, id)? {
            match b {
                0xFF => break,
                0xF0 => (line, col) = (line + 1, 0),
                0xFE => col += 1,
                _ => {
                    let (tx, ty) = (CASTLE_TEXT_AT.0 + col, CASTLE_TEXT_AT.1 + line);
                    if tx < CASTLE_BOX_W && ty < ENDING_BOX_H {
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
        png(px, sma4_assets::ending_text_path(out_dir, id))?;
    }

    m.w8(WORLD, 0)?;
    m.w8(LANGUAGE, 0)?;
    m.w8(CHARACTER, 0)?;
    m.w16(BG1_CNT, LETTER_BG1_CNT)?;
    // Sub-state 0x1F (0x080297E4) unpacks the font and the box's pieces into BG1's chars
    // (LZ77 to 0x02026000, then copied by 0x080EAA84) before the throne room comes back.
    let base = 0x0600_0000 + (LETTER_BG1_CNT as u32 & 0xC) * 0x1000;
    for (lz, copies) in FONT_LOADS {
        m.call(FN_LZ77, &[*lz, UNPACK])?;
        for &(from, to, n) in *copies {
            m.call(FN_COPY, &[from, base + to, n])?;
        }
    }
    m.call(FN_OBJ_PALETTES, &[])?;
    m.call(FN_LETTER, &[])?;
    println!("  restored throne room: messages {ids:?}, box; the letter");
    snapshot(m)
}

fn snapshot(m: &Machine) -> Result<VideoSnapshot, ExtractError> {
    let pal = m.read(PAL_BUFFER, 0x400)?;
    Ok(VideoSnapshot {
        vram: m.read(VRAM, ENDING_VRAM)?,
        palette: pal.chunks_exact(2).map(|b| u16::from_le_bytes([b[0], b[1]])).collect(),
    })
}

/// The save screen and the asset. 0x08039C30 loads the same pictures for Worlds 1-7
/// (World 8 takes the 0xE0 branch: Bowser's castle, not extracted); the world's number is
/// a sprite.
pub fn extract(rom: &Rom, m: &mut Machine, out_dir: &std::path::Path, letter: VideoSnapshot) -> Result<(), ExtractError> {
    let mut asset = EndingAsset { letter, ..Default::default() };
    for world in 0..1u8 {
        m.clear_ram()?;
        m.write(VRAM, &vec![0; ENDING_VRAM])?;
        m.w8(LANGUAGE, 0)?;
        m.w8(CHARACTER, 0)?;
        m.w8(WORLD, world)?;
        m.w8(CLEARED_WORLD, world << 5)?;
        m.w8(SUB_STATE, 0x48)?;
        m.call(FN_WORLD_CLEAR, &[])?;
        asset.clear.push(snapshot(m)?);
    }
    asset.saving = rom.decompress(rom.ptr(SAVING_LZ)?)?;
    for &(a, b) in sma4_assets::ENDING_ROM {
        asset.rom.push((a, rom.slice(a, (b - a) as usize)?.to_vec()));
    }
    asset.write(out_dir).map_err(err)?;
    println!("  world clear screens: {}", asset.clear.len());
    Ok(())
}
