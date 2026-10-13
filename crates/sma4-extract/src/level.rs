//! Builds a ready-to-draw level by running the ROM's own loaders (see docs/re-notes.md,
//! "Level storage"), then rendering every distinct metatile into an atlas.
//! Addresses are for the European ROM (AX4P rev 0).

use sma4_assets::{ANIM_FRAMES, BlockRules, EnemySpawn, LevelAsset, METATILE, METATILES, Rgba, Solidity};
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

/// Background tile animation (VBlank 0x080016DC, sources set up by 0x08016434): every 8
/// frames, 4 tiles of the question block go to char offset 0x1300 (tile 0x98) and 4 coin
/// tiles to 0x1B80 (tile 0xDC). Frame f's tiles are at offsets 0x08135F26[f] (block) and
/// 0x08135F2E[f] (coin), u16, in LZ77 0x0818B604.
const ANIM_SOURCE: u32 = 0x0818_B604;
const ANIM_BLOCK_OFFSETS: u32 = 0x0813_5F26;
const ANIM_COIN_OFFSETS: u32 = 0x0813_5F2E;
const ANIM_BLOCK_DEST: usize = 0x1300;
const ANIM_COIN_DEST: usize = 0x1B80;
const ANIM_BYTES: usize = 0x80;

/// Graphics sets without the animation (checks in 0x080016DC).
fn animates(gfx_set: u8) -> bool {
    !matches!(gfx_set, 7 | 0x10..=0x12 | 0x16 | 0x2C)
}

/// Time limit hundreds by header byte 8 >> 6 (0x08014F20): 3, 4, 2, none.
const TIME_LIMITS: u32 = 0x0813_6016;
/// Player start y per header byte 4 >> 5 (0x08014F20).
const START_Y: u32 = 0x0813_789A;

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

pub fn extract(rom: &Rom, m: &mut Machine, level_id: u16, tileset: u8, enemy_list: u16) -> Result<LevelAsset> {
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
    let mut chars = m.read(0x0600_0000, 0x8000)?;
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
    // Rendered once per animation frame, with that frame's tiles patched in.
    let anim = rom.decompress(ANIM_SOURCE)?;
    let mut atlas = Vec::with_capacity((METATILE * METATILE * 4 * METATILES * ANIM_FRAMES) as usize);
    for f in 0..ANIM_FRAMES {
        if animates(gfx_set) {
            for (offsets, dest) in [(ANIM_BLOCK_OFFSETS, ANIM_BLOCK_DEST), (ANIM_COIN_OFFSETS, ANIM_COIN_DEST)] {
                let src = rom.u16(offsets + f * 2)? as usize;
                chars[dest..dest + ANIM_BYTES].copy_from_slice(&anim[src..src + ANIM_BYTES]);
            }
        }
        for m in 0..METATILES as u16 {
            let mt = Metatile { id: m as u8, high: m >= 0x100 };
            atlas.extend(render_metatile(&chars, &palette, rom.metatile_entries(tileset, mt)?));
        }
    }

    // TODO(re): the backdrop is set by the background-scene loader (not ported). Use
    // colour 0 of the palette the sky metatile is drawn with.
    let sky_pal = rom.metatile_entries(tileset, SKY)?[0].palette as usize;
    let [r, g, b, _] = palette[sky_pal * 16].to_rgba(255);

    let t = rom.block_tables(tileset)?;
    let rules = BlockRules {
        bump_behaviour: t.bump_behaviour,
        result_hi: t.result_hi,
        item_types: t.item_types,
        final_kind: t.final_kind,
        final_metatile: t.final_metatile,
        bounce: t.bounce,
    };

    let enemies = rom
        .enemy_list(enemy_list)?
        .into_iter()
        .map(|e| EnemySpawn { kind: e.kind as u16, col: e.col as u16, row: e.row })
        .collect::<Vec<_>>();

    let distinct = metatiles.iter().collect::<std::collections::HashSet<_>>().len();
    println!("  level {level_id:#05x}: {records} objects, {width}x{ROWS} metatiles, {distinct} distinct, gfx set {gfx_set}, {} enemies", enemies.len());
    Ok(LevelAsset {
        level_id,
        tileset,
        width,
        height: ROWS,
        backdrop: [r, g, b],
        solidity: Solidity { top: solidity.top, solid: solidity.solid, high: high_ids(rom, tileset)? },
        metatiles,
        rules,
        enemies,
        time_limit: rom.u8(TIME_LIMITS + (hdr[8] >> 6) as u32)? as u16 * 100,
        start_y: rom.u16(START_Y + (hdr[4] >> 5) as u32 * 2)?,
        atlas: Rgba { width: METATILE, height: METATILE * METATILES * ANIM_FRAMES, pixels: atlas },
    })
}

fn encode(m: Metatile) -> u16 {
    m.id as u16 | if m.high { 0x100 } else { 0 }
}

/// ROM address of a level's object stream (for messages).
pub fn level_addr(rom: &Rom, level_id: u16) -> u32 {
    rom.ptr(LEVEL_TABLE + level_id as u32 * 4).unwrap_or(ROM_BASE)
}

/// Second-plane metatiles 0x00-0x25 collide like 0x0813E174[tileset][id] (0x080A06E8).
const HIGH_ID_TABLES: u32 = 0x0813_E174;

fn high_ids(rom: &Rom, tileset: u8) -> Result<[u8; sma4_assets::HIGH_IDS]> {
    let table = rom.ptr(HIGH_ID_TABLES + tileset as u32 * 4)?;
    let mut out = [0u8; sma4_assets::HIGH_IDS];
    for (i, b) in out.iter_mut().enumerate() {
        *b = rom.u8(table + i as u32)?;
    }
    Ok(out)
}
