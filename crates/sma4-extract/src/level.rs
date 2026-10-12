//! Builds a ready-to-draw level by running the ROM's own loaders (see docs/re-notes.md,
//! "Level storage"), then rendering every distinct metatile into an atlas.
//! Addresses are for the European ROM (AX4P rev 0).

use sma4_assets::{LevelAsset, METATILE, METATILES, Rgba, Solidity};
use sma4_rom::{
    ROM_BASE, Rom,
    gfx::parse_palette,
    level::{Metatile, render_metatile},
};

use crate::emu::{EmuError, Machine};

const LEVEL_TABLE: u32 = 0x0813_6200;
const HEADER_LEN: u32 = 11;

// Loader RAM variables.
const V_DATA_PTR: u32 = 0x0300_3B04;
const V_PARAM: u32 = 0x0300_3846;
const V_BANK: u32 = 0x0300_3847;
const V_ID: u32 = 0x0300_3848;
const V_TILESET: u32 = 0x0300_3849;
const V_Y: u32 = 0x0300_3AD0;
const V_X: u32 = 0x0300_3AD1;
const V_VERTICAL: u32 = 0x0300_3CB3;
const V_LEVEL_STATE: u32 = 0x0300_7818;
const V_GFX_SET: u32 = 0x0300_37DF;
const V_BG_SCENE: u32 = 0x0300_3BBF;
const V_LEVEL_ID: u32 = 0x0300_3B0A;
/// BGxCNT shadows: each layer gets its own char/screen base so the loaders don't
/// overwrite each other. Level layer (BG1) uses char block 0.
const BG_CNT_SHADOWS: [(u32, u16); 4] =
    [(0x0300_2DA4, 0x1F08), (0x0300_2DA6, 0x1C00), (0x0300_2DA8, 0x1D08), (0x0300_2DAA, 0x1E08)];

// Loader routines.
const FN_POSITION: u32 = 0x0801_4D24;
const FN_BANK0: u32 = 0x0801_4A20;
const FN_BANK1: u32 = 0x0801_49F8;
const FN_FILL_ROWS: u32 = 0x0801_4A48;
const FN_FILL_PAGES: u32 = 0x0801_4B94;
const FN_LOAD_BG_GFX: u32 = 0x0802_E860;
const FN_LOAD_PALETTES: u32 = 0x0800_85AC;

const SCRATCH_STATE: u32 = 0x0203_A000;
const PAGE_BUFFERS: u32 = 0x0202_E000; // 16 pages x 0x400 bytes of metatile ids
const HIGH_LAYER: u32 = PAGE_BUFFERS + 0x4000;
const PAL_BUFFER: u32 = 0x0200_0000;
const PAGES: usize = 16;
const ROWS: usize = 27;
const SKY: Metatile = Metatile { id: 0x80, high: false };

#[derive(Debug)]
pub struct ExtractError(pub String);

impl From<EmuError> for ExtractError {
    fn from(e: EmuError) -> Self {
        Self(e.0)
    }
}

impl From<sma4_rom::RomError> for ExtractError {
    fn from(e: sma4_rom::RomError) -> Self {
        Self(e.to_string())
    }
}

type Result<T> = std::result::Result<T, ExtractError>;

/// Replays the object loop of the level loader (0x08014F20) record by record.
fn load_objects(m: &mut Machine, rom: &Rom, data: u32, tileset: u8) -> Result<usize> {
    m.w8(V_TILESET, tileset)?;
    m.w8(V_VERTICAL, rom.u8(data + 6)? & 0x10)?;
    let mut i = data + HEADER_LEN;
    let first = rom.u8(i)?;
    if first != 0xFF && first & 0xC0 == 0xC0 {
        i += 3;
    }
    let mut records = 0;
    loop {
        let b = rom.u8(i)?;
        if b == 0xFF {
            return Ok(records);
        }
        let rec = rom.slice(i, 4)?;
        let bank = b & 0xC0;
        if bank == 0x80 {
            i += 4; // area setting, not drawn
            continue;
        }
        m.w8(V_BANK, bank)?;
        m.w8(V_PARAM, b & 0x3F)?;
        m.w8(V_Y, rec[1])?;
        m.w8(V_X, rec[2])?;
        m.w8(V_ID, rec[3])?;
        m.w8(V_TILESET, tileset)?;
        m.w32(V_DATA_PTR, i + 4)?;
        m.call(FN_POSITION, &[])?;
        if bank < 0x40 {
            m.call(FN_BANK0, &[])?;
        } else {
            m.w8(V_BANK, bank | 1)?;
            m.call(FN_BANK1, &[])?;
        }
        // Generators may consume extra bytes; the loader resumes where they leave the pointer.
        let next = m.r32(V_DATA_PTR)?;
        if !(i + 4..=i + 12).contains(&next) {
            return Err(ExtractError(format!("object at {i:#x} moved the data pointer to {next:#x}")));
        }
        i = next;
        records += 1;
    }
}

pub fn extract(rom: &Rom, m: &mut Machine, level_id: u16, tileset: u8) -> Result<LevelAsset> {
    let data = rom.ptr(LEVEL_TABLE + level_id as u32 * 4)?;
    let hdr = rom.slice(data, HEADER_LEN as usize)?.to_vec();

    m.clear_ram()?;
    m.w32(V_LEVEL_STATE, SCRATCH_STATE)?;
    // Same pre-fill as the tileset setup (0x080174EC): every page starts as sky.
    for i in 0..0xF0 {
        m.call(FN_FILL_ROWS, &[SKY.id as u32, i])?;
    }
    for i in 0..0x340 {
        m.call(FN_FILL_PAGES, &[SKY.id as u32, i])?;
    }
    let records = load_objects(m, rom, data, tileset)?;

    let gfx_set = ((hdr[8] & 0x30) << 1) | (hdr[7] & 0x1F);
    m.w8(V_TILESET, tileset)?;
    m.w8(V_GFX_SET, gfx_set)?;
    m.w8(V_BG_SCENE, hdr[10])?;
    for (addr, v) in BG_CNT_SHADOWS {
        m.w16(addr, v)?;
    }
    m.w16(V_LEVEL_ID, level_id)?;
    m.call(FN_LOAD_BG_GFX, &[])?;
    m.call(FN_LOAD_PALETTES, &[])?;

    let pages = m.read(PAGE_BUFFERS, 0x4000)?;
    let high = m.read(HIGH_LAYER, 0x4000)?;
    let palette = parse_palette(&m.read(PAL_BUFFER, 0x200)?);
    let chars = m.read(0x0600_0000, 0x8000)?;
    let at = |x: usize, y: usize| {
        let o = (x >> 4) * 0x400 + y * 16 + (x & 15);
        Metatile { id: pages[o], high: high[o] != 0 }
    };

    // Width: up to the last column containing anything but sky, rounded to whole pages.
    let last = (0..PAGES * 16).filter(|&x| (0..ROWS).any(|y| at(x, y) != SKY)).max().unwrap_or(0);
    let width = (last + 16) & !15;

    let solidity = rom.solidity(tileset)?;
    let metatiles: Vec<u16> =
        (0..ROWS).flat_map(|y| (0..width).map(move |x| (y, x))).map(|(y, x)| encode(at(x, y))).collect();

    // Every metatile of the tileset, so the game can swap cells freely (e.g. a hit
    // question block turning into an empty block). Layer = id | 0x100 * high.
    let mut atlas = Vec::with_capacity((METATILE * METATILE * 4 * METATILES) as usize);
    for m in 0..METATILES as u16 {
        let mt = Metatile { id: m as u8, high: m >= 0x100 };
        atlas.extend(render_metatile(&chars, &palette, rom.metatile_entries(tileset, mt)?));
    }

    // TODO(re): the backdrop is set by the background-scene loader (not ported). Use
    // colour 0 of the palette the sky metatile is drawn with.
    let sky_pal = rom.metatile_entries(tileset, SKY)?[0].palette as usize;
    let [r, g, b, _] = palette[sky_pal * 16].to_rgba(255);

    let distinct = metatiles.iter().collect::<std::collections::HashSet<_>>().len();
    println!("  level {level_id:#05x}: {records} objects, {width}x{ROWS} metatiles, {distinct} distinct, gfx set {gfx_set}");
    Ok(LevelAsset {
        level_id,
        tileset,
        width,
        height: ROWS,
        backdrop: [r, g, b],
        solidity: Solidity { top: solidity.top, solid: solidity.solid },
        metatiles,
        atlas: Rgba { width: METATILE, height: METATILE * METATILES, pixels: atlas },
    })
}

fn encode(m: Metatile) -> u16 {
    m.id as u16 | if m.high { 0x100 } else { 0 }
}

/// ROM address of a level's object stream (for messages).
pub fn level_addr(rom: &Rom, level_id: u16) -> u32 {
    rom.ptr(LEVEL_TABLE + level_id as u32 * 4).unwrap_or(ROM_BASE)
}
